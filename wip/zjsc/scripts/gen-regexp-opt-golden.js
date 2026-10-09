// Gera tests/golden/regexp_opt_bun.tsv: golden complementar do Yarr, focado nas otimizações do
// YarrPattern (factorAlternatives, dot-star enclosure, lookbehind com referência para frente,
// possessivo simulado por lookahead com captura, backreferences nomeadas duplicadas, class sets da
// flag v). Mesmo formato e mesmo harness de scripts/gen-regexp-golden.js: cada linha é
// fonte, KIND, REPR. As entradas são pequenas de propósito (nada patológico, sem backtracking
// exponencial): cada programa mistura casos que casam e casos que não casam.
// Uso: timeout 120 bun scripts/gen-regexp-opt-golden.js > tests/golden/regexp_opt_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const path = require("path");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/e2e_values_harness.js"), "utf8").trimEnd();

/** Literal de string JS em ASCII puro. */
function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}
const R = (pattern, flags = "") => `new RegExp(${q(pattern)}, ${q(flags)})`;

const programs = [];
const add = (source) => programs.push(source);
const exec = (pattern, flags, input) => add(`${R(pattern, flags)}.exec(${q(input)})`);

/** Produto cartesiano de padrões, flags e entradas, tudo em exec. */
function grid(patterns, flagsList, inputs) {
  for (const pattern of patterns) {
    for (const flags of flagsList) {
      for (const input of inputs) exec(pattern, flags, input);
    }
  }
}

// ---------------------------------------------------------------------------------------------
// factorAlternatives: oito ou mais alternativas com prefixo comum.
const families = [
  { prefix: "abc", tails: ["d", "e", "f", "g", "h", "i", "j", "k"] },
  { prefix: "foo", tails: ["bar", "baz", "qux", "", "b", "ba", "barn", "bazaar"] },
  { prefix: "x", tails: ["1", "12", "123", "1234", "2", "23", "234", "3", "34"] },
  { prefix: "ab", tails: ["\\d", "\\w", "[xyz]", ".", "c+", "c*", "(?:d|e)", "$"] },
  { prefix: "re", tails: ["act", "action", "ally", "ad", "alm", "ap", "lease", "member"] },
];
for (const { prefix, tails } of families) {
  const plain = tails.map((tail) => prefix + tail).join("|");
  const grouped = "(" + plain + ")";
  const captured = tails.map((tail) => prefix + "(" + tail + ")").join("|");
  const anchored = "^(?:" + plain + ")$";
  const concrete = tails.map((tail) => (tail.includes("\\") || tail.includes("[") || tail.includes("(") || tail.includes("$") || tail.includes(".") || tail.includes("*") || tail.includes("+") ? "1" : tail));
  const inputs = [
    ...concrete.slice(0, 4).map((tail) => prefix + tail),
    prefix + "zz",
    prefix.slice(0, 1),
    "",
    "zz" + prefix + concrete[1] + "zz",
    prefix.toUpperCase() + (concrete[1] || "").toUpperCase(),
  ];
  for (const pattern of [plain, grouped, captured, anchored]) {
    for (const flags of ["", "i"]) {
      for (const input of inputs) exec(pattern, flags, input);
    }
  }
}
// Prefixo comum em dois níveis, com a alternativa vazia e com captura no meio.
grid(
  ["abcd|abce|abcf|abdg|abdh|abdi|abxj|abxk|aby", "(?:ab(?:cd|ce|cf)|ab(?:dg|dh|di)|ab(?:xj|xk)|aby)", "a(b)c1|a(b)c2|a(b)c3|a(b)c4|a(b)c5|a(b)c6|a(b)c7|a(b)c8", "ab|abc|abcd|abcde|abcdef|abcdefg|abcdefgh|abcdefghi"],
  ["", "i", "u", "iu", "y"],
  ["abcd", "abdh", "abxk", "aby", "abc5", "abcdefghi", "ABCDEFGH", "xabcfx", "ab"],
);
grid(["(?:|a|ab|abc|abcd|abcde|abcdef|abcdefg)x", "(a|ab|abc|abcd|abcde|abcdef|abcdefg|abcdefgh)(c?)d?", "(?:ca|cb|cc|cd|ce|cf|cg|ch)+"], ["", "g"], ["abcdx", "abcdefghx", "cacbcc", "x", "abcdefgh"]);

// ---------------------------------------------------------------------------------------------
// /i e /u.
grid(
  ["abc|abd|abe|abf|abg|abh|abi|abj", "[a-f]+|[g-l]+", "(?:k|K)+s", "\\u017f+|s+"],
  ["i", "iu", "u", "iy"],
  ["ABD", "abJ", "KKs", "KKS", "SSsſ", "ſſ", "xyz", "ABCDEF"],
);
grid(["\\w+", "[^\\W]+", "\\b\\w+\\b"], ["i", "iu", "u"], ["ſK", "abcſ", " K "]);
grid(["^.$", "^.{2}$", "^[^a]$", "(?:\\ud83d\\ude00|x)+", "[\\ud83d\\ude00-\\ud83d\\ude4f]"], ["u", "iu", ""], ["😀", "😀x", "ab", "\ud83d", "😁😂"]);
grid(["é+|è+", "[À-ÿ]+", "σ+|ς+", "ı|i"], ["i", "iu"], ["ÉÈé", "ÿŸ", "Σςσ", "Iİiı", "abc"]);

// ---------------------------------------------------------------------------------------------
// Grupos aninhados com captura.
grid(
  ["((a)(b(c)))", "((a)|(b))+", "(?:(a)|(b)|(c))+", "(a(b(c(d(e)))))", "((a*)(b*))*", "(((a)))\\3", "((a)|b)*c", "(?:(a)(b)?)+"],
  ["", "d"],
  ["abc", "abab", "abcabc", "abcde", "aabbab", "aaa", "bbc", "ac", ""],
);
grid(["(a|ab)(c|bcd)(d*)", "((?:a|b)+)(\\1)", "(x)?(y)?(z)?", "(?:(a)|b)*?c"], ["", "i", "dg"], ["abcd", "abab", "xz", "ABAB", "bbbc", "zzz"]);

// ---------------------------------------------------------------------------------------------
// Lookbehind com referência para frente (o grupo vem depois, na leitura da direita para a esquerda).
grid(
  ["(?<=\\1(a))b", "(?<=(a)\\1)b", "(?<=\\1(\\w+))!", "(?<=(\\w)\\1)x", "(?<=\\k<n>(?<n>a))b", "(?<=(?<n>a)\\k<n>)b", "(?<!\\1(a))b", "(?<=(\\d+)\\1)x", "(?<=\\2(.)(.))c"],
  ["", "i", "u"],
  ["aab", "ab", "abab!", "xx", "aax", "1212x", "12x", "abc", "AAB", "b"],
);
grid(["(?<=a(?=b))b", "(?<=(?=a)a)b", "(?<=\\b)b", "(?<=^|,)\\w", "(?<=a.{0,2})c"], ["", "m"], ["ab", "a,b", "xab\nb", "a\nc", "aXc", "c"]);

// ---------------------------------------------------------------------------------------------
// Quantificadores possessivos simulados por lookahead com captura e backreference.
grid(
  ["(?=(a+))\\1b", "(?=(a+))\\1a", "(?=(\\d+))\\1x", "(?=(\\w+?))\\1\\d", "(?:(?=(a*))\\1)b", "(?=([^,]*))\\1,", "(?=(a|ab))\\1c", "(?=(.*?))\\1x"],
  ["", "i", "s"],
  ["aaab", "aaa", "123x", "12", "ab1", "abc", "aab", "a,b", "abc", "\nx"],
);

// ---------------------------------------------------------------------------------------------
// Dot-star enclosure, com ^ e $, flags s e m.
grid(
  [".*foo.*", "^.*foo.*$", "^.*foo", "foo.*$", ".*", ".+?x.*", "^(.*)bar(.*)$", ".*(?:foo|bar).*", "(.*)foo(.*)", "^.*$", "^.*?$", ".*\\bfoo\\b.*"],
  ["", "s", "m", "ms", "i", "is"],
  ["foo", "a foo b", "x\nfoo\ny", "foo\n", "\nfoo", "nada", "xbarx", "FOO", "a\r\nfoo\r\nb", "ab foo cd", ""],
);
grid(["^a.*b$", "^a[^]*b$", "^a[\\s\\S]*b$", "(?:.*\\n)+", "^$", "^\\s*$", "a$", "^a"], ["", "m", "s", "ms"], ["a\nb", "ab", "a\n\nb\n", "\n", "", "ba\nab", "a\r\nb"]);
grid(["(.*)(\\d)", "(.*?)(\\d)", "(.*)\\1", "((.*)\\2)", ".*?(?=b)", ".*(?<=a)"], ["", "s"], ["abc1", "a\n1b2", "abab", "xaxb", "aba\nb", "b"]);

// ---------------------------------------------------------------------------------------------
// Backreferences nomeadas duplicadas (alternativas distintas).
grid(
  [
    "(?<a>x)|(?<a>y)", "(?:(?<a>x)|(?<a>y))\\k<a>", "(?:(?<a>\\d)|(?<a>[a-z]))+", "(?<a>.)\\k<a>|(?<a>b)",
    "(?:(?<a>a)|(?<b>b))(?:(?<a>c)|(?<b>d))\\k<a>\\k<b>", "(?:(?<n>a)|b)\\k<n>", "(?:(?<n>a)|(?<n>b)|(?<n>c))\\k<n>", "(?<a>x)(?:(?<b>y)|(?<b>z))\\k<b>",
  ],
  ["", "d", "i"],
  ["x", "y", "xx", "yy", "xy", "1a", "bb", "acac", "bdbd", "aca", "aa", "ba", "xzz", "xyy", "cc", "bb"],
);
for (const [pattern, flags, input] of [
  ["(?:(?<a>x)|(?<a>y))", "", "y"],
]) {
  add(`(function () { var m = ${R(pattern, flags)}.exec(${q(input)}); return [m.groups.a, m.length, Object.keys(m.groups)] })()`);
}
add("(function () { var m = /(?:(?<a>x)|(?<a>y))/d.exec('y'); return [m.indices.groups.a, m.groups.a] })()");
add("'xy'.replace(/(?:(?<a>x)|(?<a>y))/g, '[$<a>]')");
add("'xy'.replace(/(?:(?<a>x)|(?<a>y))/g, function () { return JSON.stringify(arguments[arguments.length - 1]) })");
add("[...'xyx'.matchAll(/(?:(?<a>x)|(?<a>y))/g)].map(function (m) { return m.groups.a })");

// ---------------------------------------------------------------------------------------------
// Class sets da flag v: operações de conjunto, \q{...}, classes aninhadas.
grid(
  [
    "[a-z--[aeiou]]+", "[\\w--\\d]+", "[\\p{L}--\\p{Lu}]+", "[[a-z]&&[aeiou]]+", "[\\w&&\\d]+", "[[a-c][x-z]]+", "[^[a-c][x-z]]+", "[a-c--b]+",
    "[\\q{abc|d|ef}]+", "[\\q{abc|ab|a}]", "[\\q{ab}\\q{cd}]+", "[\\q{ab}a]", "[\\q{ab|c}--\\q{c}]", "[\\q{ab}&&\\q{ab|c}]", "[\\q{}a]", "[^\\q{a|b}]",
    "[\\p{L}&&\\q{ab|c}]", "[[a-z]--\\q{abc}]", "[\\q{abc|ab|a}--\\q{ab}]+", "^[\\q{ab|cd}]{2}$", "[\\q{ab|cd}x]+?c", "(?:[\\q{ab|cd}])\\1?",
  ],
  ["v", "iv"],
  ["hello", "ab12cd", "ABcdEF", "xyzaei", "abxyz", "abcdabc", "abcd", "ab", "c", "abab", "cdab", "xcd", "aei", ""],
);
grid(["[\\q{ß}]", "[ß]", "[\\p{Lu}--[A-Z]]", "[\\p{Ll}&&[a-z]]", "\\p{Lu}", "[^\\p{Lu}]", "[\\q{ab}--[b]]"], ["v", "iv"], ["SS", "ẞ", "ß", "À", "A", "a", "ab", "k", "K"]);
grid(["[\\p{ASCII}--[\\x00-\\x7e]]", "[\\p{Emoji_Keycap_Sequence}a]+", "\\p{RGI_Emoji_Flag_Sequence}", "[\\q{\\u{1f600}x}]"], ["v"], ["\x7f", "a1️⃣", "🇷🇴", "😀x", "x"]);

// ---------------------------------------------------------------------------------------------
// Programas que cruzam as otimizações com os métodos de String.
for (const [pattern, flags, input, replacement] of [
  ["(abc|abd|abe|abf|abg|abh|abi|abj)+", "g", "abcabdxabj", "[$1]"],
  [".*foo.*", "g", "foo\nfoo foo\nbar", "<$&>"],
  [".*foo.*", "gs", "foo\nfoo foo\nbar", "<$&>"],
  ["^.*$", "gm", "a\nb\n\nc", "|$&|"],
  ["(?:(?<a>x)|(?<a>y))", "g", "xyz", "[$<a>]"],
  ["(?=(a+))\\1", "g", "aab", "<$1>"],
  ["(?<=\\1(a))b", "g", "aabab", "X"],
  ["[\\q{ab|c}]", "gv", "abcab", "-"],
]) {
  add(`${q(input)}.replace(${R(pattern, flags)}, ${q(replacement)})`);
}
for (const [pattern, flags, input] of [
  ["abc|abd|abe|abf|abg|abh|abi|abj", "g", "abjabcxabh"],
  [".*foo.*", "gm", "foo\nxfoo\nbar"],
  ["(?:(?<a>x)|(?<a>y))\\k<a>", "g", "xxyyxy"],
  ["[a-z--[aeiou]]", "gv", "hello world"],
]) {
  add(`[...${q(input)}.matchAll(${R(pattern, flags)})].map(function (m) { return m[0] + "@" + m.index })`);
  add(`${q(input)}.split(${R(pattern, flags.replace("g", ""))})`);
}

// ---------------------------------------------------------------------------------------------
const seen = new Set();
let count = 0;
for (const src of programs) {
  if (seen.has(src)) continue;
  seen.add(src);
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  const out = (0, eval)(`${harness}(${JSON.stringify(src)})`);
  if (typeof out !== "string") throw new Error(`${src}: o harness não devolveu string`);
  emitRow(`${src}\t${out}`);
  count++;
}
process.stderr.write(`${count} programas\n`);
