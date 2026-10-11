// Gera tests/golden/regexp_legacy_bun.tsv: estáticas legadas de RegExp (`$1`..`$9`, `lastMatch`, `lastParen`,
// `leftContext`, `rightContext`, `input`, aliases), `@@replace` com `$<nome>`, `$0`, `$01`, `$10`, função com
// `groups`, `lastIndex` depois de replace, `replaceAll`, `split`, `matchAll` e o iterador, medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Uso: bun scripts/gen-regexp-legacy-golden.js > tests/golden/regexp_legacy_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const HEAD =
  "var T = f => { try { return f() } catch (e) { return e.name + ': ' + e.message } };\n" +
  "var L = () => JSON.stringify([RegExp.$1, RegExp.$2, RegExp.$3, RegExp.$4, RegExp.$5, RegExp.$6, RegExp.$7, RegExp.$8, RegExp.$9, " +
  "RegExp.lastMatch, RegExp.lastParen, RegExp.leftContext, RegExp.rightContext, RegExp.input, RegExp.$_, RegExp['$&'], RegExp['$+'], " +
  "RegExp['$`'], RegExp[\"$'\"], RegExp.multiline, RegExp['$*']]);\n";
const add = body => programs.push(HEAD + body);

// ---- 1. Legados depois de cada operação, em regex com e sem flags, em vários textos.
const regexes = [
  "/(a)(b)?/", "/(a)(b)?/g", "/(\\d+)-(\\d+)/", "/(\\d+)-(\\d+)/g", "/x/", "/(?<n>a)/y", "/a(?=(b))/i", "/(a)|(b)/",
  "/((a)(b))(c)?/m", "/\\bw(o)rd/gi", "/(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)/", "/^(\\w)/mg", "/(?:)/", "/()/g",
];
const subjects = ['"xxab yy"', '"ab"', '"12-34 56-78"', '""', '"wORd word"', '"abcabc"', '"zz"', '"a\\nb\\nc"'];
const ops = [
  "r.exec(s)", "r.test(s)", "s.match(r)", "s.replace(r, 'X')", "s.replace(r, (m, p1) => p1 + '!')", "s.split(r)",
  "s.search(r)", "[...s.matchAll(new RegExp(r.source, r.flags.includes('g') ? r.flags : r.flags + 'g'))]",
  "s.replaceAll(new RegExp(r.source, r.flags.includes('g') ? r.flags : r.flags + 'g'), '[$&]')", "r[Symbol.match](s)",
  "r[Symbol.replace](s, '<$1>')", "r[Symbol.search](s)", "r[Symbol.split](s, 2)", "r.exec(s); r.exec(s)",
];
for (const re of regexes.slice(0, 8))
  for (const subject of subjects.slice(0, 4))
    for (const op of ops.slice(0, 10)) add(`var r = ${re}, s = ${subject};\nT(() => { ${op}; });\nR = L();`);

// ---- 2. Estado anterior mantido na falha, e atualização depois de sucesso seguido de falha.
for (const op of ["/(z)/.exec('a')", "/(z)/.test('a')", "'a'.match(/(z)/)", "'a'.replace(/(z)/, 'x')", "'a'.search(/(z)/)", "'a'.split(/(z)/)",
  "/(z)/g.exec('a')", "/(z)/y.exec('a')", "'a'.match(/(z)/g)", "'a'.replaceAll(/(z)/g, 'x')"])
  add(`/(q)(r)/.exec('xqrx');\nT(() => { ${op}; });\nR = L();`);
for (const first of ["/(q)/.test('aqb')", "'aqb'.match(/(q)/)", "'aqb'.match(/(q)/g)", "'aqb'.replace(/(q)/, '')", "'aqb'.replace(/(q)/g, '')",
  "'aqb'.split(/(q)/)", "'aqb'.search(/(q)/)", "[...'aqbq'.matchAll(/(q)/g)]", "/(q)/y.exec('q')", "'aqb'.replaceAll(/(q)/g, '')"])
  add(`${first};\nR = L();`);

// ---- 3. Subclasse, new RegExp de outra regex, outro realm.
for (const op of ["r.exec('xab')", "r.test('xab')", "'xab'.match(r)", "'xab'.replace(r, '')", "'xab'.split(r)", "'xab'.search(r)", "[...'xab'.matchAll(r)]"]) {
  add(`class S extends RegExp {}\nvar r = new S('(a)(b)', 'g');\nT(() => { ${op}; });\nR = L();`);
  add(`class S extends RegExp {}\nvar r = new S('(a)(b)');\nT(() => { ${op}; });\nR = L();`);
  add(`/(q)/.exec('q');\nclass S extends RegExp {}\nvar r = new S('(a)(b)');\nT(() => { ${op}; });\nR = L();`);
  add(`var r = Reflect.construct(RegExp, ['(a)(b)'], Object);\nR = T(() => { ${op}; return L() });`);
  add(`var r = Reflect.construct(RegExp, ['(a)(b)'], Function);\nR = T(() => { ${op}; return L() });`);
  add(`var r = new RegExp(/(a)(b)/y);\nT(() => { ${op}; });\nR = L();`);
}
add("class S extends RegExp {}\nR = T(() => new S('a').compile('b').source);");
add("var r = Reflect.construct(RegExp, ['a'], Object);\nR = T(() => r.compile('b').source);");
add("class S extends RegExp {}\nvar r = new S('a');\nR = T(() => r.compile(/b/).source);");
add("class S extends RegExp {}\nR = T(() => Object.getPrototypeOf(new S('a')) === S.prototype);");
add("R = T(() => Reflect.construct(RegExp, ['a'], Object).compile('b'));");
for (const op of ["r.exec('xab')", "r.test('xab')", "'xab'.replace(r, '')", "'xab'.match(r)"]) {
  add(`var W = (0, eval)("RegExp");\nvar r = new W('(a)(b)');\nT(() => { ${op}; });\nR = L() + JSON.stringify([W.$1, W.$2, W.lastMatch, W.input]);`);
  add(`var W = (0, eval)("RegExp");\nvar r = new W('(a)(b)');\nR = T(() => { ${op}; return L() + JSON.stringify([W.$1, W.$2, W.lastMatch, W.input]) });`);
  add(`var r = (0, eval)("/(a)(b)/");\nT(() => { ${op}; });\nR = L() + (0, eval)("JSON.stringify([RegExp.$1, RegExp.$2])");`);
  add(`var r = (0, eval)("/(a)(b)/");\nR = T(() => { ${op}; return L() + (0, eval)("JSON.stringify([RegExp.$1, RegExp.lastMatch, RegExp.input])") });`);
  add(`var r = (0, eval)("/(a)(b)/g");\nvar G = (0, eval)("RegExp");\nT(() => { ${op}; });\nR = G === RegExp ? "same" : "diff";`);
}
add("R = (0, eval)('RegExp') === RegExp;");
add("(0, eval)(\"/(z)/.exec('z')\");\nR = L();");
add("/(a)/.exec('a'); (0, eval)(\"/(z)/.exec('z')\");\nR = L();");
add("var f = new Function('/(k)/.exec(\"k\")'); f();\nR = L();");

// ---- 4. Setters e propriedades das estáticas.
add("RegExp.input = 'abc';\nR = L();");
add("RegExp.$_ = 'def';\nR = L();");
add("RegExp.input = 'abc'; RegExp.$_ = 'def';\nR = RegExp.input + RegExp.$_;");
add("/(a)/.exec('xa');\nRegExp.input = 'novo';\nR = L();");
add("/(a)/.exec('xa');\nRegExp.$_ = 5;\nR = L() + typeof RegExp.input;");
add("RegExp.input = {toString() { return 'obj' }};\nR = RegExp.input;");
add("RegExp.input = Symbol();\nR = T(() => RegExp.input);");
add("R = T(() => { RegExp.input = Symbol(); return 1 });");
add("RegExp.multiline = 1;\nR = L() + typeof RegExp.multiline;");
add("RegExp['$*'] = 0;\nR = L();");
add("RegExp.multiline = true;\nR = JSON.stringify('a\\nb'.match(/^b/));");
add("RegExp['$*'] = true;\nR = JSON.stringify('a\\nb'.match(/^b/));");
add("RegExp.multiline = true;\nR = JSON.stringify('a\\nb'.match(/^b/)) + /^b/.multiline;");
add("RegExp.multiline = true;\nvar r = /^b/; R = r.test('a\\nb') + ' ' + r.multiline + ' ' + r.flags;");
add("RegExp.multiline = true;\nR = new RegExp('^b').test('a\\nb') + ' ' + new RegExp('^b').multiline;");
for (const k of ["$1", "$2", "$9", "lastMatch", "$&", "lastParen", "$+", "leftContext", "$`", "rightContext", "$'", "input", "$_", "multiline", "$*"]) {
  add(`var d = Object.getOwnPropertyDescriptor(RegExp, ${JSON.stringify(k)});\nR = JSON.stringify([typeof d.get, typeof d.set, d.enumerable, d.configurable, "writable" in d]);`);
  add(`/(a)(b)/.exec('xab');\nR = T(() => { RegExp[${JSON.stringify(k)}] = 'w'; return JSON.stringify(RegExp[${JSON.stringify(k)}]) });`);
  add(`R = T(() => { 'use strict'; RegExp[${JSON.stringify(k)}] = 'w'; return JSON.stringify(RegExp[${JSON.stringify(k)}]) });`);
  add(`R = T(() => String(delete RegExp[${JSON.stringify(k)}]));`);
  add(`R = T(() => Object.getOwnPropertyDescriptor(RegExp, ${JSON.stringify(k)}).get.call({}) + '|' + Object.getOwnPropertyDescriptor(RegExp, ${JSON.stringify(k)}).get.call(RegExp));`);
  add(`class S extends RegExp {}\n/(a)(b)/.exec('xab');\nR = T(() => JSON.stringify(S[${JSON.stringify(k)}]));`);
  add(`class S extends RegExp {}\nR = T(() => { S[${JSON.stringify(k)}] = 'z'; return JSON.stringify([S[${JSON.stringify(k)}], RegExp[${JSON.stringify(k)}]]) });`);
}
add("R = Object.keys(RegExp).length + ' ' + Object.getOwnPropertyNames(RegExp).sort().join(',');");
add("R = JSON.stringify(Object.getOwnPropertyNames(RegExp).filter(k => k[0] === '$').sort());");
add("/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)/.exec('abcdefghij');\nR = L();");
add("/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)/.exec('abcdefghij');\nR = RegExp.$10 + '|' + RegExp.$0;");
add("/(a)(?:b)(c)?/.exec('xabx');\nR = L();");
add("/(?<x>a)(?<y>b)?/.exec('a');\nR = L();");
add("/(a)|(b)/.exec('b');\nR = L();");
add("/(a)|(b)/.exec('a');\nR = L();");
add("/((a))/.exec('a');\nR = L();");
add("/(a)(b)(c)/.exec('abc');\nR = RegExp.lastParen + RegExp['$+'];");
add("/a/.exec('bab');\nR = RegExp.lastParen === '' ? 'vazio' : RegExp.lastParen;");
add("'é😀a'.match(/(a)/);\nR = L();");
add("'😀a'.match(/(\\u{1F600})/u);\nR = L();");
add("'x'.repeat(100000).match(/(x{5})/);\nR = RegExp.rightContext.length + ' ' + RegExp.leftContext.length;");
add("var s = 'abc'; /(b)/.exec(s);\ns = null;\nR = L();");
add("/(a)/.exec('a'); /(a)/.exec('a');\nR = RegExp.leftContext + '|' + RegExp.rightContext;");
add("/(a)/g.test('aa'); /(a)/g.test('aa');\nR = L();");
add("var r = /(a)/g; r.test('aa'); r.test('aa'); r.test('aa');\nR = L() + r.lastIndex;");
add("var r = /(a)/y; r.test('ba'); r.lastIndex = 1; r.test('ba');\nR = L() + r.lastIndex;");
add("var r = /(a)/y; r.test('ba');\nR = L() + r.lastIndex;");
add("var r = /(a)/gy; r.lastIndex = 5; r.test('aaa');\nR = L() + r.lastIndex;");
add("var r = /(a)/g; r.lastIndex = 1; r.exec('aaa');\nR = L() + r.lastIndex;");
add("var r = /(a)/; r.lastIndex = 2; r.exec('aaa');\nR = L() + r.lastIndex;");
add("var r = /(a)/; Object.defineProperty(r, 'lastIndex', { writable: false });\nR = T(() => { r.exec('a'); return 1 }) + L();");
add("var r = /(a)/g; Object.defineProperty(r, 'lastIndex', { writable: false });\nR = T(() => { r.exec('a'); return 1 }) + L();");
add("var r = /(a)/; r.exec = function() { return null };\nR = r.test('a') + L();");
add("var r = /(a)/; r.exec = function() { return /(z)/.exec('z') };\nR = r.test('a') + L();");
add("/(a)/.exec('a');\nvar r = /(z)/; r.exec = function() { return null };\n'z'.match(r);\nR = L();");
add("var r = /(a)/; r.exec = RegExp.prototype.exec;\n'a'.replace(r, 'x');\nR = L();");
add("RegExp.prototype.exec = function () { return null };\nvar r = /(a)/;\nR = String(r.test('a')) + L();");
add("var o = RegExp.prototype.exec; RegExp.prototype.exec = function (s) { return o.call(this, s) };\n'xa'.match(/(a)/);\nR = L();");
add("var o = RegExp.prototype.exec; RegExp.prototype.exec = function (s) { return o.call(this, s) };\n'xa'.replace(/(a)/, '');\nR = L();");
add("var o = RegExp.prototype.exec; RegExp.prototype.exec = function (s) { return o.call(this, s) };\n'xa'.split(/(a)/);\nR = L();");
add("var o = RegExp.prototype.exec; RegExp.prototype.exec = function (s) { return o.call(this, s) };\n/(a)/.test('xa');\nR = L();");
add("var o = RegExp.prototype.exec; RegExp.prototype.exec = function (s) { return o.call(this, s) };\n'xa'.search(/(a)/);\nR = L();");
add("Object.defineProperty(RegExp.prototype, 'global', { get() { return true } });\n'aa'.replace(/(a)/, '');\nR = L();");
add("Object.defineProperty(RegExp.prototype, 'flags', { get() { return 'g' } });\n'aa'.replace(/(a)/, '');\nR = L();");
add("/(a)(b)/.exec('ab');\nvar d = Object.getOwnPropertyDescriptor(RegExp, '$1');\nR = d.get.call(RegExp) + String(d.get.name) + d.get.length;");
add("R = JSON.stringify(['$1', '$9', 'lastMatch', 'input', '$_', 'multiline'].map(k => { var d = Object.getOwnPropertyDescriptor(RegExp, k); return [d.get.name, d.get.length, d.set && d.set.name, d.set && d.set.length] }));");

// ---- 5. @@replace com `$<nome>` e demais padrões de substituição.
const named = [
  ["/(?<n>a)(?<m>b)?/", "'xab y a'"], ["/(?<n>a)/g", "'aXa'"], ["/(a)(b)/", "'ab'"], ["/(?<x>a)/", "'a'"], ["/a/", "'a'"],
  ["/(?<n>\\d+)-(?<m>\\d+)/g", "'1-2 33-44'"], ["/(?<n>a)|(?<m>b)/g", "'ab'"], ["/(?:)/g", "'ab'"],
];
const subs = ["$<n>", "$<m>", "$<x>", "$<>", "$<n", "$<n>$<m>", "[$<n>]", "$<nn>", "$<N>", "$0", "$00", "$01", "$02", "$1", "$10", "$11", "$2$1", "$$",
  "$$$", "$&", "$`", "$'", "$", "$$1", "$<$1>", "x$<n>y", "$<n>>", "<$<n>", "$12", "$010", "$100", "$9"];
for (const [re, subject] of named)
  for (const sub of subs.slice(0, 20)) {
    add(`R = T(() => ${subject}.replace(${re}, ${JSON.stringify(sub)}));`);
    add(`R = T(() => ${subject}.replace(new RegExp(${re}.source, 'dg' + ${re}.flags.replace('g', '')), ${JSON.stringify(sub)}));`);
  }
for (const sub of subs.slice(0, 12))
  add(`R = T(() => 'ab'.replace(/(?<n>a)/, { toString() { return ${JSON.stringify(sub)} } }));`);
add("R = T(() => 'ab'.replaceAll('a', '$<n>'));");
add("R = T(() => 'ab'.replaceAll('a', '$&$&'));");
add("R = T(() => 'ab'.replace('a', '$1$&$`$\\'$$'));");
add("R = T(() => 'aXb'.replace('X', '$<n>'));");
add("R = T(() => 'ab'.replaceAll('', '-'));");

// ---- 6. Replacer função com argumentos.
const fnCases = [
  ["/(a)(b)?/", "'xab'"], ["/(?<n>a)(?<m>b)?/", "'xab'"], ["/(?<n>a)(?<m>b)?/g", "'a ab'"], ["/a/", "'bab'"], ["/(x)?a/", "'a'"],
  ["/(?<n>.)/g", "'é😀'"], ["/(?<n>.)/gu", "'é😀'"], ["/(?:)/g", "'ab'"], ["/(a)|(b)/g", "'ab'"],
];
for (const [re, subject] of fnCases) {
  add(`R = T(() => JSON.stringify(${subject}.replace(${re}, function () { return JSON.stringify([...arguments]) + '/' })));`);
  add(`R = T(() => { var out = []; ${subject}.replace(${re}, function () { out.push([arguments.length, typeof arguments[arguments.length - 1], arguments[arguments.length - 1] === ${subject}]) }); return JSON.stringify(out) });`);
  add(`R = T(() => { var out = []; ${subject}.replace(${re}, (...a) => { out.push(Object.getPrototypeOf(a[a.length - 1] ?? {}) === null) }); return JSON.stringify(out) });`);
  add(`R = T(() => ${subject}.replace(${re}, () => '$&$1$<n>'));`);
  add(`R = T(() => ${subject}.replace(${re}, () => undefined));`);
  add(`R = T(() => ${subject}.replace(${re}, () => ({ toString() { return 'o' } })));`);
  add(`R = T(() => ${subject}.replace(${re}, () => { throw new RangeError('boom') }));`);
  add(`R = T(() => { var order = []; ${subject}.replace(${re}, function () { order.push(RegExp.lastMatch); return '' }); return JSON.stringify(order) + RegExp.lastMatch });`);
  add(`R = T(() => ${subject}.replaceAll(${re.includes("/g") || re.endsWith("gu/") ? re : re.replace(/\/$/, "/g")}, (m, ...r) => '[' + m + r.length + ']'));`);
}
add("R = T(() => 'abc'.replace(/(?<n>b)/, (m, p1, off, str, g) => JSON.stringify([m, p1, off, str, g])));");
add("R = T(() => 'abc'.replace(/(b)/, (m, p1, off, str, g) => JSON.stringify([m, p1, off, str, g])));");
add("R = T(() => 'abc'.replace('b', (m, off, str, g) => JSON.stringify([m, off, str, g])));");
add("R = T(() => 'abcabc'.replace(/b/g, (m, off) => off));");
add("R = T(() => 'abc'.replace(/(?<n>b)/, function (...a) { return typeof a[a.length - 1] + Object.keys(a[a.length - 1]) }));");
add("R = T(() => { var re = /(?<n>b)/; re.exec = function () { return { 0: 'b', index: 1, length: 1, groups: { n: 'Z' } } }; return 'abc'.replace(re, '<$<n>>') });");
add("R = T(() => { var re = /(?<n>b)/; re.exec = function () { return { 0: 'b', index: 1, length: 1, groups: undefined } }; return 'abc'.replace(re, '<$<n>>') });");
add("R = T(() => { var re = /(?<n>b)/; re.exec = function () { return { 0: 'b', index: 1, length: 1, groups: null } }; return 'abc'.replace(re, '<$<n>>') });");
add("R = T(() => { var re = /(b)/; re.exec = function () { return { 0: 'b', index: 1, length: 1, groups: { n: 5 } } }; return 'abc'.replace(re, '<$<n>>') });");
add("R = T(() => { var re = /(b)/; re.exec = function () { return { 0: 'b', index: 1, length: 1, groups: { n: { toString() { return 'S' } } } } }; return 'abc'.replace(re, '<$<n>>') });");
add("R = T(() => { var re = /(b)/; re.exec = function () { return { 0: 'b', index: 1, length: 2, 1: 'p' } }; return 'abc'.replace(re, '<$1|$2>') });");
add("R = T(() => { var re = /(b)/; re.exec = function () { return { 0: 'b', index: -5, length: 1 } }; return 'abc'.replace(re, '!') });");
add("R = T(() => { var re = /(b)/; re.exec = function () { return { 0: 'b', index: 99, length: 1 } }; return 'abc'.replace(re, '!') });");
add("R = T(() => { var re = /(b)/; re.exec = function () { return 5 }; return 'abc'.replace(re, '!') });");

// ---- 7. lastIndex depois de replace global e sticky.
for (const flags of ["", "g", "y", "gy", "gu", "gd"]) {
  for (const subject of ["'aaa'", "''", "'baa'", "'aab'"]) {
    add(`var r = new RegExp('a', '${flags}'); r.lastIndex = 1;\nvar out = ${subject}.replace(r, 'X');\nR = JSON.stringify([out, r.lastIndex]);`);
    add(`var r = new RegExp('a', '${flags}');\nvar out = ${subject}.replace(r, 'X');\nR = JSON.stringify([out, r.lastIndex]);`);
    add(`var r = new RegExp('a*', '${flags}');\nvar out = ${subject}.replace(r, 'X');\nR = JSON.stringify([out, r.lastIndex]);`);
    add(`var r = new RegExp('a', '${flags}'); r.lastIndex = 5;\nvar out = ${subject}.replace(r, () => 'F');\nR = JSON.stringify([out, r.lastIndex]);`);
    add(`var r = new RegExp('a', '${flags}');\nvar out = ${subject}.match(r);\nR = JSON.stringify([out, r && r.lastIndex, r.lastIndex]);`);
    add(`var r = new RegExp('a', '${flags}'); r.lastIndex = 2;\nvar out = ${subject}.search(r);\nR = JSON.stringify([out, r.lastIndex]);`);
    add(`var r = new RegExp('a', '${flags}'); r.lastIndex = 2;\nvar out = ${subject}.split(r);\nR = JSON.stringify([out, r.lastIndex]);`);
  }
}
add("var r = /a/g; Object.defineProperty(r, 'lastIndex', { writable: false, value: 0 });\nR = T(() => 'aa'.replace(r, 'x'));");
add("var r = /a/y; Object.defineProperty(r, 'lastIndex', { writable: false, value: 0 });\nR = T(() => 'aa'.replace(r, 'x'));");
add("var r = /a/; Object.defineProperty(r, 'lastIndex', { writable: false, value: 0 });\nR = T(() => 'aa'.replace(r, 'x'));");
add("var r = /a/g; r.lastIndex = { valueOf() { return 1 } };\nvar o = 'aaa'.replace(r, 'x');\nR = o + typeof r.lastIndex + r.lastIndex;");
add("var r = /a/y; r.lastIndex = -1;\nvar o = 'aaa'.replace(r, 'x');\nR = o + r.lastIndex;");
add("var r = /a/y; r.lastIndex = 1;\nvar o = 'aaa'.replace(r, 'x');\nR = o + r.lastIndex;");
add("var r = /a/y; r.lastIndex = 1;\nvar o = r.exec('aaa');\nR = JSON.stringify([o, r.lastIndex, o.index]);");
add("var r = /a/gy; var o = 'aaab'.replace(r, 'x');\nR = o + r.lastIndex;");
add("var r = /a/gy; var o = 'baa'.replace(r, 'x');\nR = o + r.lastIndex;");
add("var r = /a/gy; var o = 'aaab'.match(r);\nR = JSON.stringify(o) + r.lastIndex;");

// ---- 8. replaceAll com regex não global e demais erros.
for (const re of ["/a/", "/a/y", "/a/i", "/a/d", "/a/u", "/a/v", "/a/m", "/a/s", "/a/gi", "/a/gy"])
  add(`R = T(() => 'aa'.replaceAll(${re}, 'x'));`);
add("R = T(() => 'aa'.replaceAll({ [Symbol.match]: true, flags: 'g', [Symbol.replace]() { return 'ok' } }, 'x'));");
add("R = T(() => 'aa'.replaceAll({ [Symbol.match]: true, flags: 'i', [Symbol.replace]() { return 'ok' } }, 'x'));");
add("R = T(() => 'aa'.replaceAll({ [Symbol.match]: true, flags: undefined, [Symbol.replace]() { return 'ok' } }, 'x'));");
add("R = T(() => 'aa'.replaceAll({ [Symbol.match]: true, flags: null, [Symbol.replace]() { return 'ok' } }, 'x'));");
add("R = T(() => 'aa'.replaceAll({ [Symbol.match]: true, [Symbol.replace]() { return 'ok' } }, 'x'));");
add("R = T(() => 'aa'.replaceAll(/a/g, 'x'));");
add("R = T(() => 'aa'.replaceAll('a', 'x'));");
add("R = T(() => 'aa'.replaceAll(null, 'x'));");
add("R = T(() => String.prototype.replaceAll.call(null, /a/g, 'x'));");
add("R = T(() => String.prototype.replace.call(undefined, /a/g, 'x'));");
add("var r = /a/g; Object.defineProperty(r, 'flags', { value: 'i' });\nR = T(() => 'aa'.replaceAll(r, 'x'));");
add("var r = /a/; Object.defineProperty(r, 'flags', { value: 'g' });\nR = T(() => 'aa'.replaceAll(r, 'x'));");
add("var r = /a/g; r[Symbol.match] = false;\nR = T(() => 'aa'.replaceAll(r, 'x'));");
add("var r = /a/; r[Symbol.match] = false;\nR = T(() => 'a/a/'.replaceAll(r, 'x'));");
add("R = T(() => RegExp.prototype[Symbol.replace].call(1, 'a', 'b'));");
add("R = T(() => RegExp.prototype[Symbol.replace].call({}, 'a', 'b'));");
add("R = T(() => RegExp.prototype[Symbol.replace].call(/a/, Symbol(), 'b'));");
add("R = T(() => RegExp.prototype[Symbol.replace].call(/a/, 'a', Symbol()));");
add("R = T(() => RegExp.prototype[Symbol.match].call(1, 'a'));");
add("R = T(() => RegExp.prototype[Symbol.matchAll].call(1, 'a'));");
add("R = T(() => RegExp.prototype[Symbol.split].call(1, 'a'));");
add("R = T(() => RegExp.prototype[Symbol.search].call(1, 'a'));");
add("R = T(() => 'a'.matchAll(/a/));");
add("R = T(() => 'a'.matchAll(/a/y));");
add("R = T(() => 'a'.matchAll(/a/g).next().value[0]);");
add("R = T(() => 'a'.matchAll('a').next().value[0]);");
add("R = T(() => 'a'.matchAll(null).next().value[0]);");
add("R = T(() => 'a'.matchAll(undefined).next().value[0]);");
add("R = T(() => String.prototype.matchAll.call(null, /a/g));");
add("R = T(() => 'a'.match(null));");
add("R = T(() => 'null'.match(null)[0]);");
add("R = T(() => 'undefined'.match(undefined)[0]);");

// ---- 9. split com regex, captura e limite.
const splitCases = [
  ["'a1b2c3'", "/\\d/"], ["'a1b2c3'", "/(\\d)/"], ["'a1b2c3'", "/(\\d)(x)?/"], ["'abc'", "/(?:)/"], ["'abc'", "/()/"], ["''", "/a/"], ["''", "/(?:)/"],
  ["'abc'", "/b/y"], ["'abc'", "/b/g"], ["'abc'", "/B/i"], ["'a\\nb'", "/^/m"], ["'😀a'", "/(?:)/u"], ["'😀a'", "/(?:)/"], ["'aXbXc'", "/(X)/"],
  ["'aXbXc'", "/(?<n>X)/"], ["'test'", "/(?:)/"], ["'ab'", "/a*?/"], ["'ab'", "/a*/"], ["'A<B>bold</B>and<CODE>coded</CODE>'", "/<(\\/)?([^<>]+)>/"],
];
for (const [subject, re] of splitCases)
  for (const limit of ["", ", 0", ", 1", ", 2", ", -1", ", 4294967297"])
    add(`R = T(() => JSON.stringify(${subject}.split(${re}${limit})));`);
add("var r = /a/; r.constructor = { [Symbol.species]: function () { return /b/ } };\nR = T(() => JSON.stringify('abab'.split(r)));");
add("class S extends RegExp { static get [Symbol.species]() { return RegExp } }\nR = T(() => JSON.stringify('a1b'.split(new S('\\\\d'))));");
add("var r = /\\d/; r.constructor = undefined;\nR = T(() => JSON.stringify('a1b'.split(r)));");
add("var r = /\\d/; r.constructor = 1;\nR = T(() => JSON.stringify('a1b'.split(r)));");
add("var r = /\\d/; r.constructor = { [Symbol.species]: undefined };\nR = T(() => JSON.stringify('a1b'.split(r)));");
add("var r = /\\d/; r.constructor = { [Symbol.species]: null };\nR = T(() => JSON.stringify('a1b'.split(r)));");

// ---- 10. matchAll com lastIndex inicial e o iterador.
for (const li of ["0", "1", "2", "3", "4", "-1", "1.5", "'1'"])
  for (const re of ["/a/g", "/a/gy", "/(?:)/g", "/(?<n>a)/g", "/a/gd"]) {
    add(`var r = ${re}; r.lastIndex = ${li};\nvar it = T(() => 'aaa'.matchAll(r));\nR = typeof it === 'string' ? it : JSON.stringify([...it].map(m => [m[0], m.index, m.groups && m.groups.n])) + r.lastIndex;`);
    add(`var r = ${re}; r.lastIndex = ${li};\nvar it = 'aaa'.matchAll(r);\nR = JSON.stringify([it.next().value && 1, r.lastIndex]);`);
    add(`var r = ${re}; r.lastIndex = ${li};\nvar it = 'aaa'.matchAll(r);\nR = JSON.stringify([r.lastIndex, ${JSON.stringify("")} + Array.from(it).length, r.lastIndex]);`);
  }
add("var it = 'a'.matchAll(/a/g);\nR = it[Symbol.toStringTag] + '|' + Object.prototype.toString.call(it);");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nR = P[Symbol.toStringTag] + '|' + Object.getOwnPropertyNames(P).join() + '|' + Object.getOwnPropertySymbols(P).map(String).join();");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nvar d = Object.getOwnPropertyDescriptor(P, Symbol.toStringTag);\nR = JSON.stringify([d.value, d.writable, d.enumerable, d.configurable]);");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nR = (Object.getPrototypeOf(P) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))) + '|' + typeof P[Symbol.iterator] + '|' + P.next.name + P.next.length;");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nvar d = Object.getOwnPropertyDescriptor(P, 'next');\nR = JSON.stringify([d.writable, d.enumerable, d.configurable]);");
add("var it = 'a'.matchAll(/a/g);\nR = (it[Symbol.iterator]() === it) + '';");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nR = T(() => P.next.call({}));");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nR = T(() => P.next.call([][Symbol.iterator]()));");
add("var P = Object.getPrototypeOf('a'.matchAll(/a/g));\nR = T(() => P.next.call(undefined));");
add("var it = 'aa'.matchAll(/a/g); it.next(); it.next();\nR = JSON.stringify([it.next(), it.next()]);");
add("var it = 'aa'.matchAll(/a/g);\nR = JSON.stringify(it.next());");
add("var it = 'a'.matchAll(/a/g);\nR = it.constructor === Object + '' + Object.getPrototypeOf(it).constructor;");
add("var it = 'a'.matchAll(/a/g);\nR = Object.getOwnPropertyNames(it).length + ' ' + Reflect.ownKeys(it).length;");
add("R = T(() => new (Object.getPrototypeOf('a'.matchAll(/a/g)).next.constructor)('return 1')());");
add("var it = 'a'.matchAll(/a/g); var sp = RegExp.prototype[Symbol.matchAll];\nR = sp.name + sp.length + T(() => sp.call(/a/g, 'a').next().value[0]);");
add("class S extends RegExp { static get [Symbol.species]() { return function (p, f) { return new RegExp(p, f + 'd') } } }\nR = T(() => JSON.stringify([...'aa'.matchAll(new S('a', 'g'))].map(m => !!m.indices)));");
add("class S extends RegExp { static get [Symbol.species]() { return RegExp } }\nvar r = new S('a', 'g'); r.lastIndex = 1;\nR = T(() => JSON.stringify([...'aaa'.matchAll(r)].map(m => m.index)) + r.lastIndex);");
add("var r = /a/g; r.flags;\nvar calls = [];\nvar p = new Proxy(r, { get(t, k, rc) { calls.push(String(k)); var v = Reflect.get(t, k, t); return typeof v === 'function' ? v.bind(t) : v } });\nT(() => [...RegExp.prototype[Symbol.matchAll].call(p, 'a')]);\nR = calls.join();");
add("R = T(() => JSON.stringify([...'a1b2'.matchAll(/(?<d>\\d)/g)].map(m => [m[0], m.index, m.input, JSON.stringify(m.groups)])));");
add("R = T(() => JSON.stringify([...'😀😀'.matchAll(/(?:)/g)].map(m => m.index)));");
add("R = T(() => JSON.stringify([...'😀😀'.matchAll(/(?:)/gu)].map(m => m.index)));");
add("R = T(() => JSON.stringify(Array.from('abc'.matchAll(/./g), m => m[0])));");
add("'abc'.matchAll(/(b)/g).next();\nR = L();");
add("[...'abab'.matchAll(/(b)/g)];\nR = L();");
add("var it = 'abab'.matchAll(/(b)/g); it.next();\nR = L();");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "regexp-legacy-golden-"));
const file = path.join(dir, "regexp_legacy_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
const lines = [];
let kept = 0;
let dropped = 0;
for (const body of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 15000 });
  const marked = run.stdout.split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body) + "\n");
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("regexp_legacy", lines));
fs.rmSync(dir, { recursive: true, force: true });
