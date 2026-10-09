// Gera tests/golden/regexp_modern_bun.tsv: RegExp moderno medido no bun (JavaScriptCore).
// Cobre a flag v (notação de conjuntos, \q{}, propriedades de string, subtração e interseção, erros de sintaxe),
// a flag d (indices e groups em indices), grupos nomeados duplicados em alternativas, lookbehind com referências
// de volta, \p{} com Script/Script_Extensions/General_Category em grade de caracteres astrais, case-insensitive com
// u e v, sticky com lastIndex em grade, Symbol.replace com $<name>, $&, $`, $' e grupos ausentes, RegExp.escape e
// modificadores (?i:). Programas cuja expressão já aparece em algum golden regexp*.tsv existente são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Um bun filho novo por programa,
// em paralelo, com timeout de 5 s. Nenhum programa lê estado estático de RegExp entre programas.
// O prelúdio comum sai em tests/golden/regexp_modern.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-regexp-modern-golden.js > tests/golden/regexp_modern_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function M(m){if(m===null)return "null";var o={a:Array.from(m),i:m.index,g:m.groups===undefined?"u":Object.entries(m.groups)};if(m.indices){o.d=Array.from(m.indices);o.dg=m.indices.groups===undefined?"u":Object.entries(m.indices.groups)}return JSON.stringify(o)}\n' +
  'function B(re,list){return list.map(function(c){re.lastIndex=0;return re.test(typeof c==="number"?String.fromCodePoint(c):c)?1:0}).join("")}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const mk = (p, f) => `new RegExp(${q(p)},${q(f)})`;

// ---- 1. Flag v: operações de conjunto em grade (operando x operando x operação), testadas numa grade de caracteres.
const grid = ["a", "b", "z", "A", "1", "_", "-", "é", "ß", "ſ", "K", "\u212a", "\u{1F600}", "\u{10400}", "\u{10428}", "\u0130", "\u0131", " ", "\u{1D7CE}", "\u{1F1E6}"];
const operands = ["a-z", "\\p{L}", "\\p{Lu}", "\\d", "\\w", "\\s", "\\P{L}", "[a-c]", "[b-d]", "\\p{ASCII}", "\\p{Script=Latin}", "\\p{Emoji}", "[aeiou]", "\\q{a|bc}", "\\q{ß|ss}", "[^a-m]", "\\p{Ll}", "\\p{N}", "[\\p{L}--\\p{Lu}]", "\\u{1F600}-\\u{1F64F}"];
const asWrap = o => (o.startsWith("[") || o.startsWith("\\p") || o.startsWith("\\P") || o.startsWith("\\d") || o.startsWith("\\w") || o.startsWith("\\s") || o.startsWith("\\q") ? o : "[" + o + "]");
for (const a of operands) for (const b of operands) {
  for (const op of ["--", "&&"]) {
    const pat = `^[${asWrap(a)}${op}${asWrap(b)}]$`;
    add(`T(()=>B(${mk(pat, "v")},${q(grid)}))`);
  }
}
for (const a of operands.slice(0, 12)) for (const b of operands.slice(0, 12)) {
  add(`T(()=>B(${mk(`^[${asWrap(a)}${asWrap(b)}]$`, "v")},${q(grid)}))`);
  add(`T(()=>B(${mk(`^[^${asWrap(a)}${asWrap(b)}]$`, "vi")},${q(grid)}))`);
}
for (const a of operands) for (const f of ["v", "vi", "vs", "vm", "vg", "vy"]) {
  add(`T(()=>B(${mk(`^${asWrap(a)}$`, f)},${q(grid)}))`, `T(()=>B(${mk(`^[^${a.startsWith("[") ? a.slice(1, -1) : a}]$`, f)},${q(grid)}))`);
}

// ---- 2. \q{} e propriedades de string.
const qpats = ["[\\q{abc}]", "[\\q{abc|d}]", "[\\q{}]", "[\\q{|a}]", "[\\q{a|ab|abc}]", "[\\q{ab}a]", "[a\\q{ab}]", "[\\q{\\u{1F600}x}]", "[\\q{ß}]", "[\\q{ss|ß}]", "[\\q{a-b}]", "[\\q{\\q{a}}]",
  "[^\\q{a}]", "[^\\q{ab}]", "[^\\q{a|b}]", "[\\q{ab}--\\q{a}]", "[\\q{ab|c}--\\q{c}]", "[\\q{ab|c}&&\\q{c|d}]", "[\\q{abc|ab|a}--[a]]", "[[a-c]--\\q{b}]", "[\\q{ab}&&[ab]]", "[\\q{\\d}]", "[\\q{\\n|\\t}]",
  "[\\q{\\}}]", "[\\q{ab}c]d", "(?:[\\q{ab|a}])b", "[\\q{ab|a}]+", "[\\q{a|ab}]{2}", "^[\\q{abc|ab|a}]$", "[\\q{ab}]*?c", "[\\q{xy}\\q{z}]"];
const qinputs = ["", "a", "ab", "abc", "abab", "b", "c", "d", "ss", "ß", "SS", "\u{1F600}x", "\n", "ab\n", "abcd", "ac", "xyz", "zxy"];
for (const p of qpats) {
  add(`T(()=>String(${mk(p, "v")}))`, `T(()=>${mk(p, "v")}.source)`, `T(()=>${mk(p, "u")}.source)`);
  for (const f of ["v", "vi"]) for (const i of qinputs) add(`T(()=>M(${mk(p, f)}.exec(${q(i)})))`);
}
const strProps = ["RGI_Emoji", "Basic_Emoji", "Emoji_Keycap_Sequence", "RGI_Emoji_Flag_Sequence", "RGI_Emoji_Modifier_Sequence", "RGI_Emoji_Tag_Sequence", "RGI_Emoji_ZWJ_Sequence", "Emoji", "Emoji_Presentation", "Emoji_Modifier", "Emoji_Modifier_Base", "Emoji_Component", "Extended_Pictographic"];
const sinputs = ["\u{1F600}", "\u{1F1E7}\u{1F1F7}", "\u{1F1E7}", "1\uFE0F\u20E3", "1\u20E3", "#\uFE0F\u20E3", "\u{1F44D}\u{1F3FD}", "\u{1F3FD}", "\u{1F468}\u200D\u{1F469}\u200D\u{1F467}", "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}", "\u2764\uFE0F", "\u2764", "\u00A9", "\u00A9\uFE0F", "a", "*", "\u{1F9D1}\u200D\u{1F4BB}", "\u263A\uFE0F", "\u{1F1E7}\u{1F1F7}\u{1F1E6}", "0\uFE0F\u20E3"];
for (const p of strProps) {
  for (const i of sinputs) {
    add(`T(()=>M(${mk(`\\p{${p}}`, "v").replace(/\)$/, ")")}.exec(${q(i)})))`, `T(()=>M(${mk(`^\\p{${p}}$`, "v")}.exec(${q(i)})))`, `T(()=>M(${mk(`^[\\p{${p}}]$`, "v")}.exec(${q(i)})))`);
  }
  add(`T(()=>${mk(`\\p{${p}}`, "u")})`, `T(()=>${mk(`\\p{${p}}`, "")}.source)`, `T(()=>${mk(`\\P{${p}}`, "v")})`, `T(()=>${mk(`[^\\p{${p}}]`, "v")})`, `T(()=>${mk(`[\\P{${p}}]`, "v")})`, `T(()=>${mk(`\\p{${p}}`, "vi")}.flags)`,
    `T(()=>String(${mk(`\\p{${p}=Yes}`, "v")}))`, `T(()=>String(${mk(`\\p{${p}=}`, "v")}))`, `T(()=>${mk(`[\\p{${p}}--\\q{a}]`, "v")}.test("a"))`);
}

// ---- 3. Erros de sintaxe e fontes da flag v (mensagens exatas).
const errPats = ["[a--]", "[--a]", "[a--b--c]", "[a&&b&&c]", "[a--b&&c]", "[a&&b--c]", "[a-z--b]", "[a-z&&b]", "[a&&&b]", "[a&&]", "[&&a]", "[a--b-c]", "[(]", "[)]", "[{]", "[}]", "[/]", "[-]", "[|]", "[a-]", "[-a]", "[a-b-]", "[&&]", "[--]", "[a&b]", "[a&&&&b]",
  "[\\q]", "[\\q{]", "[\\q{a]", "\\q{a}", "[\\q{a}", "[q{a}]", "[[]", "[[a]", "[[a]]", "[[]]", "[]]", "[^]", "[^^]", "[[^a]--b]", "[\\p{L}-z]", "[a-\\p{L}]", "[\\d-z]", "[a-\\d]", "[\\w--\\d]", "[!!]", "[##]", "[$$]", "[%%]", "[**]", "[++]", "[,,]", "[..]", "[::]", "[;;]", "[<<]", "[==]", "[>>]", "[??]", "[@@]", "[^^^]", "[``]", "[~~]",
  "[a&&b", "[a--b", "[\\q{a|}]", "[\\q{a||b}]", "[\\p{RGI_Emoji}]", "[^\\p{RGI_Emoji}]", "[^\\q{ab}]", "[^[\\q{ab}]]", "[^[\\q{a}]]", "[^[a]\\q{ab}]", "[\\P{RGI_Emoji}]", "\\P{RGI_Emoji}", "[\\p{Basic_Emoji}--\\q{a}]", "\\p{Foo}", "\\p{Script=Foo}", "\\p{Script}", "\\p{=Latin}", "\\p{L=Lu}", "\\p{General_Category=}",
  "\\p{ Lu}", "\\p{lu}", "\\p{Lu }", "\\p{gc=Lu}", "\\p{GC=Lu}", "\\p{Script=latin}", "\\p{sc=Latn}", "\\p{scx=Latn}", "\\p{Script_Extensions=Latin}", "\\p{Block=Basic_Latin}", "\\p{InBasic_Latin}", "\\p{IsLatin}", "\\p{Latin}", "\\p{Any}", "\\p{ASCII}", "\\p{Assigned}", "\\p{ASCII_Hex_Digit}", "\\p{AHex}", "\\p{Alpha}", "\\p{Alphabetic}", "\\p{Lowercase_Letter}", "\\p{LC}", "\\p{Cased_Letter}", "\\p{Letter}", "\\p{Other}", "\\p{Cn}", "\\p{Unassigned}", "\\p{Co}", "\\p{Cs}", "\\p{Surrogate}",
  "(?<a>x)(?<a>y)", "(?<a>x)|(?<a>y)", "(?<a>x)|(?<b>y)|(?<a>z)", "(?:(?<a>x)|(?<a>y))(?<a>z)", "(?<a>x)(?:(?<a>y)|z)", "(?:(?<a>x)|(?<a>y))\\k<a>", "(?<a>x)|\\k<b>", "\\k<a>", "\\k<a>(?<a>x)", "(?<a>x)\\k", "(?<a>x)\\k<", "(?<a>x)\\k<a", "(?<>x)", "(?<1a>x)", "(?<a-b>x)", "(?<\\u0061>x)", "(?<\\u{61}>x)", "(?<a\\u{62}>x)", "(?<\u{1D4D0}>x)", "(?<$>x)", "(?<_>x)",
  "(?i:a)", "(?-i:a)", "(?i-s:a)", "(?i-i:a)", "(?ii:a)", "(?-:a)", "(?:a)", "(?i)", "(?i:", "(?g:a)", "(?m:a)", "(?s:a)", "(?x:a)", "(?u:a)", "(?v:a)", "(?y:a)", "(?d:a)", "(?is-m:a)", "(?im-s:a)", "(?ims-:a)", "(?-ims:a)",
  "a{2,1}", "a{1,2}{3}", "a**", "a+*", "a?+", "(?=a)*", "(?=a){2}", "(?<=a)*", "(?!a)+", "(?<!a)?", "\\1", "(a)\\2", "\\u{110000}", "\\u{10FFFF}", "\\u{}", "\\u{1F600", "\\c", "\\cA", "\\c1", "\\_", "\\-", "\\a", "\\e", "\\ ", "\\/", "\\=", "\\!", "\\:", "\\,", "\\@", "\\0", "\\00", "\\01", "\\8", "\\x", "\\x4", "\\x41", "\\u", "\\u00", "\\u0041", "{", "}", "]", "a{", "a{1", "a{1,", "a{,1}", "x{1}{2}"];
for (const p of errPats) for (const f of ["v", "u", "vi", "dv", "uv", "vu", ""]) add(`T(()=>String(${mk(p, f)}))`);
for (const f of ["v", "uv", "vu", "dgimsuvy", "dgimsvy", "vv", "gg", "vgi", "z", "V", "yy", "dd", "ii", "imsx", "uu", "vd", "dvs", "yvmi", "", " ", "v ", "\u0076", "gimsuyd", "gimsuyv"]) {
  add(`T(()=>new RegExp("a",${q(f)}).flags)`, `T(()=>String(new RegExp("a",${q(f)})))`, `T(()=>{var r=new RegExp("a",${q(f)});return [r.global,r.ignoreCase,r.multiline,r.dotAll,r.unicode,r.unicodeSets,r.sticky,r.hasIndices].join()})`);
}

// ---- 4. Flag d: indices e groups em indices.
const dcases = [
  ["(a)(b)?", "a"], ["(a)(b)?", "ab"], ["(?<x>a)(?<y>b)?", "a"], ["(?<x>a)(?<y>b)?", "ab"], ["(?<x>a)|(?<y>b)", "b"], ["(?<x>a)|(?<y>b)", "a"], ["(?<x>)", ""], ["(?<x>a*)", "aaa"], ["(a)\\1", "aa"], ["(?<n>a)\\k<n>", "aa"],
  ["((a)|(b))+", "ab"], ["((a)|(b))+", "ba"], ["(?:(a)|b)*", "ab"], ["(?=(a))", "a"], ["(?<=(a))b", "ab"], ["(?<!(a))b", "cb"], ["(?!(a))b", "b"], ["\\u{1F600}(.)", "\u{1F600}x"], ["(.)", "\u{1F600}"], ["(?<e>.)", "\u{1F600}"],
  ["(a)|b", "b"], ["(a|b)*", "abab"], ["(?<z>a)(?<y>b)(?<x>c)", "abc"], ["(\\d+)-(\\d+)", "x 12-345 y"], ["(?<year>\\d{4})-(?<mon>\\d\\d)", "on 2024-05-01"], ["^(?<a>.)(?<b>.)$", "\u{1F600}\u{1F601}"], ["", ""], ["", "abc"], ["a*?", "aaa"], ["(a*)*", "b"], ["(a*)+", "b"], ["(?<w>\\w+)\\s(?<v>\\w+)", "hello world"],
  ["(?<__proto__>a)", "a"], ["(?<constructor>a)", "a"], ["(?<toString>a)", "a"], ["(?<a>a)(?<b>b)(?<c>c)(?<d>d)", "abcd"],
];
for (const [p, i] of dcases) for (const f of ["d", "dg", "dy", "du", "dv", "di", "ds", "dgu", "dgv", "dm", ""]) {
  add(`T(()=>M(${mk(p, f)}.exec(${q(i)})))`);
}
for (const [p, i] of dcases.slice(0, 20)) {
  add(`T(()=>{var m=${mk(p, "d")}.exec(${q(i)});return m&&[m.indices.length,Object.keys(m.indices).join(),Object.keys(m).join(),Object.getPrototypeOf(m.indices)===Array.prototype,m.indices.groups===undefined?"u":Object.getPrototypeOf(m.indices.groups)].join("|")})`,
    `T(()=>{var m=${mk(p, "d")}.exec(${q(i)});return m&&Object.getOwnPropertyNames(m).join()})`,
    `T(()=>${q(i)}.match(${mk(p, "dg")}).map(String).join("|"))`, `T(()=>[...${q(i)}.matchAll(${mk(p, "dg")})].map(m=>JSON.stringify(m.indices)).join("|"))`,
    `T(()=>JSON.stringify(${mk(p, "d")}.exec(${q(i)})&&${mk(p, "d")}.exec(${q(i)}).indices.groups))`);
}
add(`T(()=>new RegExp("a","d").hasIndices)`, `T(()=>RegExp.prototype.hasIndices)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"hasIndices").get.call(/a/d))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"hasIndices").get.call({}))`,
  `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"hasIndices").get.name)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"unicodeSets").get.call(RegExp.prototype))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"unicodeSets").get.call(/a/v))`,
  `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"unicodeSets").get.call({}))`, `T(()=>RegExp.prototype.flags)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"flags").get.call({hasIndices:1,global:1,unicodeSets:1,sticky:0,dotAll:1}))`,
  `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,"flags").get.call({hasIndices:1,unicode:1,unicodeSets:1,ignoreCase:1,multiline:1}))`, `T(()=>Object.getOwnPropertyNames(RegExp.prototype).join())`);

// ---- 5. Grupos nomeados duplicados em alternativas.
const dupPats = ["(?<a>x)|(?<a>y)", "(?:(?<a>x)|(?<a>y))z", "(?<a>x)|(?<a>y)|(?<a>z)", "(?:(?<a>x)|(?<b>y))|(?<a>z)", "(?<a>x)|(?:(?<a>y)|(?<a>z))", "(?:(?<a>\\d)|(?<a>[a-z]))+", "(?:(?<a>\\d)|(?<a>[a-z]))\\k<a>", "(?<a>x)|(?<a>y)\\k<a>", "(?:(?<a>x)\\k<a>|(?<a>y)\\k<a>)",
  "(?<a>x)(?:|(?<a>y))", "(?:(?<a>x)|y)(?:(?<b>z)|w)", "^(?:(?<d>\\d+)|(?<d>[a-z]+))$", "(?:(?<a>.)|(?<a>..))$", "(?:(?<a>a)|(?<a>b))(?:(?<a>c)|(?<a>d))", "(?:(?<a>a)|(?<b>b))(?:(?<a>c)|(?<b>d))", "(?:(?<a>x)|(?<a>y))(?<=\\k<a>)", "(?<=(?<a>x)|(?<a>y))z", "(?=(?<a>x)|(?<a>y))."];
const dupIn = ["x", "y", "z", "xz", "yz", "xy", "1", "a", "1a", "a1", "xx", "yy", "ac", "bd", "ad", "bc", "xxyy", "ab", "123", "abc", ""];
for (const p of dupPats) {
  for (const f of ["", "d", "u", "v", "g", "dgu"]) for (const i of dupIn.slice(0, 12)) add(`T(()=>M(${mk(p, f)}.exec(${q(i)})))`);
  add(`T(()=>${q("xyz")}.replace(${mk(p, "g")},"[$<a>]"))`, `T(()=>${q("xyz1a")}.replace(${mk(p, "g")},(...a)=>JSON.stringify(a[a.length-1])))`, `T(()=>[...${q("xyzxy")}.matchAll(${mk(p, "g")})].map(m=>JSON.stringify(m.groups)).join("|"))`);
}

// ---- 6. Lookbehind com referências de volta.
const lbPats = ["(?<=(a)\\1)b", "(?<=\\1(a))b", "(?<=(a))\\1b", "(?<=(.)\\1)x", "(?<=\\1(.))x", "(?<=(?<n>.)\\k<n>)x", "(?<=\\k<n>(?<n>.))x", "(?<!(.)\\1)x", "(?<!\\1(.))x", "(?<=(\\d+)(\\d+))x", "(?<=(\\d+?)(\\d+))x", "(?<=(?:(a)|b)\\1)c", "(?<=\\1(?:(a)|b))c", "(?<=(a)|b)\\1c",
  "(?<=(a*)b\\1)c", "(?<=\\1b(a*))c", "(?<=(a+)\\1)c", "(?<=\\1(a+))c", "(?<=([ab])\\1)c", "(?<=\\1([ab]))c", "(?<=(.)(.)\\2\\1)x", "(?<=\\2\\1(.)(.))x", "(?<=(.)(?<=\\1))x", "(?<=a(?=(b))\\1)c", "(?<=(?<=(a))b)c", "(?<=(?<!(a))b)c", "(?<=b(a)?)\\1c", "(?<=(a)?b)\\1c", "(?<=(?:(a)|(b))\\2)c", "(?<=\\2(?:(a)|(b)))c"];
const lbIn = ["aabx", "abx", "aaax", "aax", "ax", "xx", "112x", "1212x", "aac", "aabc", "abbc", "baac", "bbc", "abab", "abbax", "baabx", "bac", "abc", "bb", "aabbc", "aaaac", "aaac", "ababc", "x", "abcc", "cc", "abxx", "aaxx", "bbx"];
for (const p of lbPats) for (const i of lbIn) add(`T(()=>M(${mk(p, "d")}.exec(${q(i)})))`);
for (const p of lbPats.slice(0, 14)) for (const f of ["u", "v", "i", "iu"]) for (const i of lbIn.slice(0, 8)) add(`T(()=>M(${mk(p, f)}.exec(${q(i)})))`);

// ---- 7. \p{} em grade astral: Script, Script_Extensions, General_Category.
const cps = [0x41, 0x3b1, 0x10000, 0x10028, 0x10330, 0x10400, 0x10428, 0x10480, 0x10800, 0x10900, 0x10D30, 0x11000, 0x11100, 0x11200, 0x12000, 0x13000, 0x16A40, 0x16F00, 0x1B000, 0x1D100, 0x1D400, 0x1D7CE, 0x1E900, 0x1E922, 0x1F000, 0x1F1E6, 0x1F300, 0x1F3FB, 0x1F600, 0x1F900, 0x1FA70, 0x20000, 0x2A6DF, 0x2B740, 0x2F800, 0x30000, 0xE0001, 0xE0100, 0xF0000, 0x10FFFF, 0x1D306, 0x1D360, 0x1F100, 0x1F200];
const scripts = ["Latin", "Greek", "Cyrillic", "Han", "Common", "Inherited", "Gothic", "Deseret", "Linear_B", "Osage", "Old_Italic", "Phoenician", "Brahmi", "Cuneiform", "Egyptian_Hieroglyphs", "Adlam", "Hiragana", "Katakana", "Arabic", "Hebrew", "Unknown", "Zzzz", "Latn", "Grek", "Qaai", "Zyyy", "Zinh", "Hani", "Kana", "Hira", "Old_Hungarian", "Hanifi_Rohingya", "Kaithi", "Chakma", "Tangut", "Nushu", "Khitan_Small_Script", "Vithkuqi", "Toto", "Kawi"];
for (const s of scripts) for (const k of ["Script", "sc", "Script_Extensions", "scx"]) {
  for (const f of ["u", "v"]) add(`T(()=>B(${mk(`^\\p{${k}=${s}}$`, f)},${JSON.stringify(cps)}))`);
  add(`T(()=>B(${mk(`^\\P{${k}=${s}}$`, "u")},${JSON.stringify(cps)}))`);
}
const gcs = ["L", "Lu", "Ll", "Lt", "Lm", "Lo", "LC", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "S", "Sm", "Sc", "Sk", "So", "Z", "Zs", "Zl", "Zp", "C", "Cc", "Cf", "Cs", "Co", "Cn", "Letter", "Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter", "Modifier_Letter", "Other_Letter", "Cased_Letter", "Mark", "Nonspacing_Mark", "Spacing_Mark", "Enclosing_Mark", "Number", "Decimal_Number", "digit", "Letter_Number", "Other_Number", "Punctuation", "punct", "Connector_Punctuation", "Dash_Punctuation", "Open_Punctuation", "Close_Punctuation", "Initial_Punctuation", "Final_Punctuation", "Other_Punctuation", "Symbol", "Math_Symbol", "Currency_Symbol", "Modifier_Symbol", "Other_Symbol", "Separator", "Space_Separator", "Line_Separator", "Paragraph_Separator", "Other", "Control", "cntrl", "Format", "Surrogate", "Private_Use", "Unassigned", "Combining_Mark"];
for (const g of gcs) {
  add(`T(()=>B(${mk(`^\\p{${g}}$`, "u")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\p{gc=${g}}$`, "u")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\p{General_Category=${g}}$`, "v")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\P{${g}}$`, "v")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^[^\\p{${g}}]$`, "u")},${JSON.stringify(cps)}))`);
}
const binProps = ["Alphabetic", "Any", "ASCII", "ASCII_Hex_Digit", "Assigned", "Bidi_Control", "Bidi_Mirrored", "Case_Ignorable", "Cased", "Changes_When_Casefolded", "Changes_When_Casemapped", "Changes_When_Lowercased", "Changes_When_NFKC_Casefolded", "Changes_When_Titlecased", "Changes_When_Uppercased", "Dash", "Default_Ignorable_Code_Point", "Deprecated", "Diacritic", "Emoji", "Emoji_Component", "Emoji_Modifier", "Emoji_Modifier_Base", "Emoji_Presentation", "Extended_Pictographic", "Extender", "Grapheme_Base", "Grapheme_Extend", "Hex_Digit", "IDS_Binary_Operator", "IDS_Trinary_Operator", "ID_Continue", "ID_Start", "Ideographic", "Join_Control", "Logical_Order_Exception", "Lowercase", "Math", "Noncharacter_Code_Point", "Pattern_Syntax", "Pattern_White_Space", "Quotation_Mark", "Radical", "Regional_Indicator", "Sentence_Terminal", "Soft_Dotted", "Terminal_Punctuation", "Unified_Ideograph", "Uppercase", "Variation_Selector", "White_Space", "XID_Continue", "XID_Start", "Alpha", "AHex", "Hex", "IDC", "IDS", "Ideo", "Lower", "Upper", "space", "RI", "VS", "EPres", "ExtPict", "EBase", "EMod", "EComp", "CWCF", "CWCM", "CWKCF", "CWL", "CWT", "CWU", "DI", "Dep", "Dia", "Ext", "Gr_Base", "Gr_Ext", "NChar", "QMark", "SD", "STerm", "Term", "UIdeo", "XIDC", "XIDS", "Bidi_C", "Bidi_M", "CI", "Join_C", "LOE", "Pat_Syn", "Pat_WS", "IDSB", "IDST", "Cased", "Math", "Dash", "Radical"];
for (const p of binProps) add(`T(()=>B(${mk(`^\\p{${p}}$`, "u")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\p{${p}}$`, "v")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\P{${p}}$`, "v")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\p{${p}}$`, "ui")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^\\P{${p}}$`, "ui")},${JSON.stringify(cps)}))`, `T(()=>B(${mk(`^[^\\p{${p}}]$`, "vi")},${JSON.stringify(cps)}))`);

// ---- 8. Case-insensitive com u e v (folding), em grade de pares.
const foldChars = ["a", "A", "k", "K", "\u212a", "s", "S", "\u017f", "\u00df", "\u1e9e", "\u03c3", "\u03c2", "\u03a3", "\u0130", "\u0131", "i", "I", "\u00b5", "\u03bc", "\u039c", "\u01c4", "\u01c5", "\u01c6", "\u1c90", "\u10d0", "\u2126", "\u03c9", "\u03a9", "\u00e5", "\u212b", "\u00c5", "\ufb00", "\ufb06", "\ufb05", "\u{10400}", "\u{10428}", "\u{1E900}", "\u{1E922}", "\u0345", "\u03b9", "\u1fbe", "\u0399", "\u01f0", "\u0149", "\u1e9b", "\u1e61", "\u1e60", "\u03b2", "\u03d0", "\u03b8", "\u03d1", "\u03f4", "\u03d5", "\u03c6", "\u03a6", "\u03f0", "\u03ba", "\u03d6", "\u03c0", "\u03f1", "\u03c1", "\u1e9e", "\u2c2f", "\u2c5f", "\ua64a", "\ua64b", "\u1c88", "\ua64b"];
for (const c of foldChars) {
  const grid2 = foldChars;
  for (const f of ["i", "iu", "iv", "u", "v"]) {
    add(`T(()=>B(${mk(`^${c === "\\" ? "\\\\" : c}$`, f)},${q(grid2)}))`);
    add(`T(()=>B(${mk(`^[${c}]$`, f)},${q(grid2)}))`);
    if (f !== "u") add(`T(()=>B(${mk(`^[^${c}]$`, f)},${q(grid2)}))`);
  }
  add(`T(()=>B(${mk(`^\\P{Lu}$`, "iu")},${q([c])}))`, `T(()=>B(${mk(`^[^\\P{Lu}]$`, "iv")},${q([c])}))`, `T(()=>B(${mk(`^\\P{Ll}$`, "iv")},${q([c])}))`, `T(()=>B(${mk(`^[\\p{Lu}--${c}]$`, "iv")},${q(grid2)}))`, `T(()=>B(${mk(`^[\\p{Ll}&&${c}]$`, "iv")},${q(grid2)}))`, `T(()=>B(${mk(`^\\w$`, "iu")},${q([c])}))`, `T(()=>B(${mk(`^\\W$`, "iu")},${q([c])}))`, `T(()=>B(${mk(`^\\W$`, "iv")},${q([c])}))`, `T(()=>B(${mk(`^[^\\W]$`, "iv")},${q([c])}))`, `T(()=>B(${mk(`^\\b$`, "iu")},${q([c])}))`);
}
for (const c of foldChars.slice(0, 40)) for (const d of foldChars.slice(0, 40)) {
  add(`T(()=>${mk(`^${c}$`, "iu")}.test(${q(d)})+","+${mk(`^[${c}]$`, "iv")}.test(${q(d)})+","+${mk(`^${c}$`, "i")}.test(${q(d)})`);
}
add(`T(()=>${mk("[a-z]", "iu")}.test("\u212a")`, `T(()=>${mk("[a-z]", "i")}.test("\u212a")`, `T(()=>${mk("\\w", "iu")}.test("\u212a")`, `T(()=>${mk("\\w", "i")}.test("\u017f")`, `T(()=>${mk("\\W", "iu")}.test("S")`, `T(()=>${mk("\\W", "iv")}.test("S")`, `T(()=>${mk("\\W", "i")}.test("S")`,
  `T(()=>${mk("[^\\W]", "iu")}.test("\u017f")`, `T(()=>${mk("[\\W]", "iu")}.test("\u017f")`, `T(()=>${mk("[^\\W]", "iv")}.test("\u017f")`, `T(()=>${mk("[\\W]", "iv")}.test("\u017f")`, `T(()=>${mk("\\b", "iu")}.test("\u017f")`, `T(()=>${mk("\\B", "iu")}.test("\u017f")`);

// ---- 9. Sticky com lastIndex em grade.
const stickyPats = ["a", "a*", "a+", "\\d+", "(?:)", "^a", "a$", "\\ba", "(?<=x)a", "a|b", "(a)(b)?", "\\u{1F600}", ".", "[^]", "$", "x*?", "a{2}"];
const stickyIn = ["aaa", "xa", "ab", "a1\n", "\u{1F600}a", "a\u{1F600}", "123a", ""];
for (const p of stickyPats) for (const f of ["y", "yg", "yu", "yv", "yd", "ym", "yi"]) for (const i of stickyIn) for (const li of [0, 1, 2, 3, 5, -1]) {
  add(`T(()=>{var r=${mk(p, f)};r.lastIndex=${li};var m=r.exec(${q(i)});return M(m)+"|"+r.lastIndex})`);
}
for (const p of stickyPats.slice(0, 8)) for (const f of ["y", "yg"]) {
  add(`T(()=>${q("aaxaa")}.replace(${mk(p, f)},"-"))`, `T(()=>{var r=${mk(p, f)};r.lastIndex=2;return ${q("aaxaa")}.replace(r,"-")+r.lastIndex})`, `T(()=>${q("aaxaa")}.split(${mk(p, f)}).join("|")`, `T(()=>${q("aaxaa")}.match(${mk(p, f)}).join("|")`, `T(()=>${q("aaxaa")}.search(${mk(p, f)})`,
    `T(()=>{var r=${mk(p, f)};r.lastIndex=3;return ${q("aaxaa")}.search(r)+","+r.lastIndex})`, `T(()=>[...${q("aaxaa")}.matchAll(${mk(p, "yg")})].map(m=>m.index).join())`);
}
for (const li of [-5, -0, 0.5, 1.9, "2", "x", NaN, Infinity, 2 ** 32, 2 ** 53, null, undefined, { valueOf() { return 1 } }, { valueOf() { throw new RangeError("li") } }]) {
  const lis = typeof li === "object" && li !== null ? (li.valueOf.toString().includes("throw") ? "{valueOf(){throw new RangeError('li')}}" : "{valueOf(){return 1}}") : li === undefined ? "undefined" : Object.is(li, -0) ? "-0" : typeof li === "number" && !isFinite(li) ? String(li) : JSON.stringify(li);
  for (const f of ["y", "g", "", "yg"]) add(`T(()=>{var r=${mk("a", f)};r.lastIndex=${lis};var m=r.exec("aaa");return M(m)+"|"+S(r.lastIndex)})`, `T(()=>{var r=${mk("a", f)};r.lastIndex=${lis};var b=r.test("aaa");return b+"|"+S(r.lastIndex)})`);
}
add(`T(()=>{var r=/a/y;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return r.exec("b")})`, `T(()=>{var r=/a/y;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("a"))})`, `T(()=>{var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("a"))})`,
  `T(()=>{var r=/a/;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("a"))})`, `T(()=>{var r=/a/;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("b"))})`, `T(()=>{"use strict";var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("b"))})`);

// ---- 10. Symbol.replace com $<name>, $&, $`, $' e grupos ausentes.
const reps = ["$&", "$`", "$'", "$$", "$1", "$2", "$3", "$01", "$10", "$00", "$0", "$<a>", "$<b>", "$<c>", "$<>", "$<a", "$<a>$<b>", "[$<a>|$<b>]", "$<a>$", "$$&", "$$$", "$$1", "$&$&", "<$`|$'>", "$<z>", "$<constructor>", "$ ", "$x", "$a", "$-", "$<a>$1$2", "$11", "$21", "$<a>$<a>", "x$", "$<", "$<>>", "$$<a>", "$<a$>", "$<A>"];
const repSubjects = [["(?<a>x)(?<b>y)?", "1xy2"], ["(?<a>x)(?<b>y)?", "1x2"], ["(?<a>x)|(?<b>y)", "1y2x"], ["(x)(y)?", "1xy2"], ["(x)(y)?", "1x2"], ["x", "1x2x3"], ["(?<a>x)", "1x2"], ["(x)", "1x2"], ["", "ab"], ["(?<a>)", "ab"], ["(?<a>b)?c", "ac"], ["(b)?c", "ac"], ["(?<a>.)(?<b>.)(?<c>.)?", "\u{1F600}ab"], ["(?<a>a)(?<b>b)(?<c>c)(?<d>d)(?<e>e)(?<f>f)(?<g>g)(?<h>h)(?<i>i)(?<j>j)(?<k>k)", "abcdefghijk"]];
for (const [p, s] of repSubjects) for (const r of reps) {
  add(`T(()=>${q(s)}.replace(${mk(p, "")},${q(r)}))`, `T(()=>${q(s)}.replace(${mk(p, "g")},${q(r)}))`);
}
for (const [p, s] of repSubjects.slice(0, 8)) for (const f of ["u", "v", "gu", "gv", "y", "gy", "d", "gd", "gi"]) for (const r of ["$&|$<a>|$1", "$`$'", "$2$<b>"]) {
  add(`T(()=>${q(s)}.replace(${mk(p, f)},${q(r)}))`);
}
for (const [p, s] of repSubjects) {
  add(`T(()=>${q(s)}.replace(${mk(p, "g")},function(){return JSON.stringify(Array.from(arguments).map(function(a){return typeof a==="object"&&a!==null?Object.entries(a):a}))}))`,
    `T(()=>${q(s)}.replaceAll(${mk(p, "g")},"<$&>"))`, `T(()=>${q(s)}.replaceAll(${mk(p, "")},"<$&>"))`, `T(()=>${q(s)}.split(${mk(p, "")}).map(String).join("|"))`, `T(()=>${q(s)}.split(${mk(p, "u")}).map(String).join("|"))`,
    `T(()=>${q(s)}.split(${mk(p, "v")},2).map(String).join("|"))`, `T(()=>${q(s)}.search(${mk(p, "v")})`, `T(()=>${q(s)}.match(${mk(p, "gv")}))`, `T(()=>[...${q(s)}.matchAll(${mk(p, "gv")})].map(m=>m.index+":"+m[0]).join())`);
}
add(`T(()=>/(?<a>x)/[Symbol.replace]("x","$<a>$<a>")`, `T(()=>/(?<a>x)/[Symbol.replace]("x",function(m,p1,off,str,groups){return JSON.stringify(groups)})`, `T(()=>/(x)/[Symbol.replace]("x",function(m,p1,off,str,groups){return arguments.length+","+groups})`,
  `T(()=>/(?<a>x)/[Symbol.replace]("x",function(){return arguments.length})`, `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:0,length:1,0:"x",groups:{a:"G"}}},flags:""},"x","$<a>"))`,
  `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:0,length:1,0:"x",groups:undefined}},flags:""},"x","$<a>"))`, `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:0,length:1,0:"x",groups:null}},flags:""},"x","$<a>"))`,
  `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:0,length:1,0:"x",groups:1}},flags:""},"x","$<a>"))`, `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:0,length:2,0:"x",1:undefined,groups:{a:undefined}}},flags:""},"x","[$1|$<a>]"))`,
  `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:5,length:1,0:"x"}},flags:""},"abc","[$&]"))`, `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:-3,length:1,0:"x"}},flags:""},"abc","[$&]"))`,
  `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return {index:1,length:1,0:"bcdef"}},flags:""},"abc","[$&|$'|$\`]"))`);

// ---- 11. RegExp.escape.
const escInputs = ["", "a", "abc", "a.b", "a*b", "a+b?", "(x)", "[x]", "{1}", "a|b", "^$", "\\", "/", "-", "--", "a-b", ",", " ", "\n", "\t", "\r", "\u2028", "\u2029", "\ufeff", "\u00a0", "1abc", "9", "_", "_x", "\u00e9", "\u{1F600}", "\ud800", "\udc00", "\ud800a", "!", "\"", "#", "%", "&", "'", ":", ";", "<", "=", ">", "@", "`", "~", "0", "a0", "A", "Z", "z", "\0", "\x01", "\x7f", "\x80", "\u0085", "\u1680", "\u2000", "\u200b", "\u202f", "\u205f", "\u3000", "ab\ncd", "x y", "a,b", "$&", "(?:x)", "\\d", "\\\\"];
for (const s of escInputs) add(`T(()=>RegExp.escape(${q(s)}))`);
add(`T(()=>RegExp.escape.length)`, `T(()=>RegExp.escape.name)`, `T(()=>typeof RegExp.escape)`, `T(()=>RegExp.escape(1))`, `T(()=>RegExp.escape(undefined))`, `T(()=>RegExp.escape(null))`, `T(()=>RegExp.escape({}))`, `T(()=>RegExp.escape(["a"]))`, `T(()=>RegExp.escape(Symbol()))`, `T(()=>RegExp.escape())`, `T(()=>RegExp.escape(new String("a.b")))`,
  `T(()=>Object.getOwnPropertyDescriptor(RegExp,"escape").enumerable)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp,"escape").writable)`, `T(()=>Object.getOwnPropertyDescriptor(RegExp,"escape").configurable)`, `T(()=>new RegExp.escape("a"))`,
  `T(()=>RegExp.escape.call(null,"a.b"))`, `T(()=>Object.getOwnPropertyNames(RegExp).filter(function(k){return k==="escape"}).join())`);
for (const s of escInputs) for (const f of ["", "u", "v"]) {
  if (f === "v" || f === "u") add(`T(()=>{var e=RegExp.escape(${q(s)});return new RegExp("^"+e+"$",${q(f)}).test(${q(s)})})`);
  else add(`T(()=>{var e=RegExp.escape(${q(s)});return new RegExp("^"+e+"$").test(${q(s)})})`);
}

// ---- 12. Modificadores (?i:), (?-i:), (?s:), (?m:).
const modPats = ["(?i:a)b", "(?i:a)B", "(?-i:a)b", "(?i:a)(?-i:b)", "(?-i:a)b", "(?i-s:.)", "(?s:.)", "(?-s:.)", "(?m:^a$)", "(?-m:^a$)", "(?m-i:^a$)", "(?i:[a-z])", "(?i:[^a-z])", "(?i:\\w)", "(?i:\\u212a)", "(?i:k)", "(?i:(a)\\1)", "(?i:(?<n>a)\\k<n>)", "(?i:a(?-i:b)c)", "(?i:a(?-i:b)c)D", "(?:(?i:a))b", "(?i:a|b)c", "(?i:ab)+", "(?i:a)*", "(?s:a.b)(?-s:c.d)", "(?ims:a)", "(?i-i:a)", "(?i:\\p{Lu})", "(?i:\\P{Lu})", "(?i:[\\p{Lu}--[A-Z]])", "(?-i:\\p{Lu})", "(?i:\\ba)", "(?i:(?=A))a", "(?i:(?<=A))a", "(?i:\\u{10400})", "(?i:\\u0130)", "(?i:\\u0131)", "(?i:i)", "(?i:ß)", "(?i:ſ)", "(?i:s)", "(?:(?i:s)|k)"];
const modIn = ["ab", "AB", "aB", "Ab", "a\nb", "A", "a", "b", "B", "K", "\u212a", "k", "kk", "aa", "AA", "aA", "a\n", "x\na\nx", "\u{10428}", "\u{10400}", "\u0130", "i", "I", "\u0131", "\u00df", "\u017f", "S", "s", "abD", "abd", "aBcD", "abcD", "abcd", "ab\nc\nd", "x"];
for (const p of modPats) {
  add(`T(()=>String(${mk(p, "")}))`, `T(()=>String(${mk(p, "u")}))`, `T(()=>String(${mk(p, "v")}))`);
  for (const f of ["", "u", "v", "i", "s", "m"]) for (const i of modIn.slice(0, 18)) add(`T(()=>M(${mk(p, f)}.exec(${q(i)})))`);
}
// Quantidade grande de entradas exploradas sem a flag.
for (const p of modPats.slice(0, 14)) for (const i of modIn.slice(18)) add(`T(()=>M(${mk(p, "d")}.exec(${q(i)})))`);

// ---- Execução.
const baseTexts = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  if (!/^regexp/.test(file) || file === "regexp_modern_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try {
      const src = JSON.parse(line.split("\t")[0]);
      const i = src.indexOf("globalThis.R = ");
      baseTexts.add(i >= 0 ? src.slice(i + 15) : src);
    } catch (e) {}
  }
}
const seen = new Set();
const balanced = e => {
  for (const cand of [e, e + ")"]) {
    try { new Function("T", "S", "M", "B", "return " + cand); return cand; } catch (err) {}
  }
  return null;
};
const unique = exprs.map(balanced).filter(e => e && !seen.has(e) && seen.add(e));
const sources = [];
let dup = 0;
for (const expr of unique) {
  if (baseTexts.has(expr)) { dup++; continue; }
  sources.push([expr, '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`]);
}

function runChild(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 5000, killSignal: "SIGKILL" });
    let out = "";
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", () => {});
    child.on("error", () => resolve(null));
    child.on("close", code => resolve(code === 0 ? decodeResult(out) : null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(sources.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (next < sources.length) {
      const idx = next++;
      results[idx] = await runChild(sources[idx][1]);
    }
  });
  await Promise.all(workers);
  let kept = 0, dropped = 0;
  const lines = [];
  for (let i = 0; i < sources.length; i++) {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || /[\u2013\u2014]/.test(result) || /[\u2013\u2014]/.test(sources[i][1])) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(sources[i][0]).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    lines.push({ source: sources[i][1], result: result });
  }
  process.stdout.write(emitFactored("regexp_modern", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
