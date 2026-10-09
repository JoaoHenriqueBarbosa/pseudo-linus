// Gera tests/golden/node_path_bun.tsv: `require("path")` medido no bun 1.4.2 (forma do módulo, nomes e `length` das funções,
// join, normalize, resolve com primeiro caminho absoluto, isAbsolute, relative entre caminhos absolutos, dirname, basename com
// ext, extname, parse, format, toNamespacedPath, erros ERR_INVALID_ARG_TYPE de cada função), do posix e do `path.win32`
// (letras de unidade, UNC, `\\?\`, relative sem diferenciar maiúsculas, identidades cruzadas posix/win32).
// `matchesGlob` fica fora enquanto não existir o `Bun.Glob` no porte (ele é implementado em cima do glob do bun). Nada
// depende do diretório corrente de quem gera: os casos win32 só usam caminhos absolutos ou com raiz própria.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-node-path-golden.js > tests/golden/node_path_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var P = require('path');\n" +
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

// Forma do módulo.
expr(`Object.keys(P).filter(function (k) { return k !== 'matchesGlob' })`);
expr(`P.sep + P.delimiter`);
expr(`P.posix === P`);
expr(`P.win32.win32 === P.win32`);
expr(`P.win32.posix === P`);
expr(`P.posix.win32 === P.win32`);
expr(`P.posix.posix === P`);
expr(`P._makeLong === P.toNamespacedPath`);
for (const f of ["resolve", "normalize", "isAbsolute", "join", "relative", "toNamespacedPath", "dirname", "basename", "extname", "format", "parse"]) {
  expr(`[P.${f}.name, P.${f}.length, typeof P.${f}]`);
  expr(`(function (d) { return [d.enumerable, d.writable, d.configurable] })(Object.getOwnPropertyDescriptor(P, '${f}'))`);
}

const strings = ["", ".", "..", "/", "//", "///", "a", "a/", "/a", "/a/", "a/b", "a//b", "a/./b", "a/../b", "../a", "../../a", "a/b/..", "a/b/../..", "a/b/../../..",
  "/a/b/..", "/..", "/../a", "./a", "./", "a/b/c/", "/a/b/c", "...", "/a/.../b", ".a", "a.", ".a.b", "a.b.c", "/a/b.txt", "/a/.b", "/a/b/", "a//", "é/ü", "a b/c d"];
for (const s of strings) {
  const q = JSON.stringify(s);
  for (const f of ["normalize", "isAbsolute", "dirname", "basename", "extname", "parse"]) expr(`P.${f}(${q})`);
  expr(`P.join(${q})`);
  expr(`P.join('x', ${q})`);
  expr(`P.resolve('/base', ${q})`);
  expr(`P.resolve(${q}, '/z')`);
  expr(`P.toNamespacedPath(${q})`);
}
for (const [path, ext] of [["/a/b.txt", ".txt"], ["/a/b.txt", "txt"], ["/a/b.txt", "b.txt"], ["/a/b", "b"], ["b", "b"], ["/a/b/", "b"], ["/a/bb", "b"], ["a.js", ".js"], ["a.js.js", ".js"], ["/a/b", ""], ["/a/b", "/a/b"], ["/a/b", "longer/than/path"], ["aaa", "aa"], ["/x/.js", ".js"]]) {
  expr(`P.basename(${JSON.stringify(path)}, ${JSON.stringify(ext)})`);
}
for (const parts of [[], [""], ["", ""], ["a", "b"], ["a", "", "b"], ["/a", "../.."], ["a", "/b"], ["a/", "/b/"], ["..", "a"], [".", "."], ["a", "..", ".."], ["//a", "b"], ["a/b", "../../.."]]) {
  expr(`P.join(${parts.map((p) => JSON.stringify(p)).join(", ")})`);
}
for (const parts of [["/"], ["/a", "b"], ["/a", "/b"], ["/a", "/b", "c"], ["/a/b", "..", ".."], ["/a/b", "../../.."], ["x", "/a", "", "b"], ["/a", "", ""], ["/a/", "b/"], ["/a", "./b", "../c"]]) {
  expr(`P.resolve(${parts.map((p) => JSON.stringify(p)).join(", ")})`);
}
for (const [from, to] of [["/a/b/c", "/a/d"], ["/a", "/a/b/c"], ["/a/b", "/a"], ["/", "/a"], ["/a", "/"], ["/", "/"], ["/aaa/bbb", "/aaa/bbbb"], ["/aaa/bbbb", "/aaa/bbb"], ["/a/b", "/a/b"], ["/a/b/", "/a/b"], ["/a//b", "/a/c"], ["/a/b/c/d", "/a/x/y"], ["/a/b/../c", "/a/c"], ["/foo/bar", "/foo/bar/baz"], ["/foo/bar/baz", "/foo/bar"], ["/foo", "/bar"], ["/", "/a/b/c"], ["/a/b/c", "/"]]) {
  expr(`P.relative(${JSON.stringify(from)}, ${JSON.stringify(to)})`);
}
for (const obj of ["{}", "{ base: 'a' }", "{ root: '/', base: 'a' }", "{ root: '/', dir: '/x', base: 'a' }", "{ dir: 'x', base: 'a' }", "{ dir: 'x' }", "{ dir: '/' , root: '/' }", "{ root: '/' }",
  "{ name: 'n', ext: '.e' }", "{ name: 'n' }", "{ ext: '.e' }", "{ name: 'n', ext: 'e' }", "{ base: 'a', name: 'n', ext: '.e' }", "{ dir: 'x', name: 'n', ext: '.e' }", "{ dir: 1 }", "{ base: 1 }",
  "{ root: 0, dir: '', base: 'a' }", "{ dir: null, base: 'a' }", "{ dir: 'a/b', root: 'a' }", "[]", "function () {}", "new String('x')"]) {
  expr(`P.format(${obj})`);
}
for (const v of ["1", "undefined", "null", "{}", "[]", "true", "Symbol()", "10n", "() => 1"]) {
  for (const f of ["normalize", "isAbsolute", "dirname", "basename", "extname", "parse", "join", "resolve", "relative", "format"]) expr(`P.${f}(${v})`);
  expr(`P.join('a', ${v})`);
  expr(`P.resolve('a', ${v})`);
  expr(`P.resolve(${v}, '/a')`);
  expr(`P.relative('/a', ${v})`);
  expr(`P.basename('a', ${v})`);
  expr(`P.toNamespacedPath(${v}) === (${v}) || (typeof (${v}) === 'number' && isNaN(${v}))`);
}
expr(`P.basename('a', undefined)`);
expr(`P.join('a', 'b', 3, 'c')`);
expr(`P.resolve(1, 2, '/x')`);
expr(`P.resolve('/x', 1)`);

// path.win32. Os literais JS usam `\\` para a barra invertida; `q` é o JSON da string, que já escapa.
expr(`Object.keys(P.win32).filter(function (k) { return k !== 'matchesGlob' })`);
expr(`P.win32.sep + P.win32.delimiter`);
expr(`P.win32._makeLong === P.win32.toNamespacedPath`);
expr(`P.win32.toNamespacedPath === P.toNamespacedPath`);
for (const f of ["resolve", "normalize", "isAbsolute", "join", "relative", "toNamespacedPath", "dirname", "basename", "extname", "format", "parse"]) {
  expr(`[P.win32.${f}.name, P.win32.${f}.length, typeof P.win32.${f}]`);
  expr(`(function (d) { return [d.enumerable, d.writable, d.configurable] })(Object.getOwnPropertyDescriptor(P.win32, '${f}'))`);
}
const winStrings = ["", ".", "..", "\\", "/", "\\\\", "//", "a", "a\\", "\\a", "a\\b", "a/b", "a\\\\b", "a\\.\\b", "a\\..\\b", "..\\a", "a\\b\\..\\..\\..", "C:", "c:", "C:\\", "C:/", "C:a", "C:\\a", "C:/a/b", "C:\\a\\..\\..\\b", "C:\\a\\b\\", "C:.", "C:..", "1:\\a",
  "\\\\srv", "\\\\srv\\", "\\\\srv\\share", "\\\\srv\\share\\", "\\\\srv\\share\\a", "//srv/share/x/../y", "\\\\srv\\\\share\\\\a", "\\\\?\\C:\\a", "\\\\?\\UNC\\srv\\share\\a", "\\\\.\\C:\\a", "\\\\.\\pipe\\x",
  "a:b", "a:\\b", "x\\a:b\\..", "C:\\a\\b.txt", "C:\\.b", "C:a.b", "C:.a", "a.b.c", ".a", "a.", "..", "\\a\\b.tar.gz", "é\\ü", "a b\\c d"];
for (const s of winStrings) {
  const q = JSON.stringify(s);
  for (const f of ["normalize", "isAbsolute", "dirname", "basename", "extname", "parse"]) expr(`P.win32.${f}(${q})`);
  expr(`P.win32.join(${q})`);
  expr(`P.win32.join('x', ${q})`);
  expr(`P.win32.join(${q}, 'x')`);
  expr(`P.win32.resolve(${q}, 'C:\\\\z')`);
  expr(`P.win32.resolve(${q}, '\\\\\\\\srv\\\\sh\\\\z')`);
  if (/^(?:[a-zA-Z]:[\\/]|\\\\|\/\/)/.test(s)) expr(`P.win32.toNamespacedPath(${q})`);
  else expr(`P.win32.toNamespacedPath(${q}) === ${q} || typeof P.win32.toNamespacedPath(${q})`);
}
for (const [path, ext] of [["C:\\a\\b.txt", ".txt"], ["C:\\a\\b.txt", "txt"], ["C:\\a\\b.txt", "b.txt"], ["C:\\a\\b", "b"], ["b", "b"], ["C:\\a\\b\\", "b"], ["C:\\a\\bb", "b"], ["C:b", "b"], ["C:", "C:"], ["C:a.js", ".js"], ["a/b.js", ".js"], ["a\\b\\", ".js"], ["C:\\a\\b", ""], ["C:\\a\\b", "longer\\than\\path"], ["aaa", "aa"], ["C:\\x\\.js", ".js"]]) {
  expr(`P.win32.basename(${JSON.stringify(path)}, ${JSON.stringify(ext)})`);
}
for (const parts of [[], [""], ["", ""], ["a", "b"], ["a", "", "b"], ["C:\\a", "..\\.."], ["a", "C:\\b"], ["a\\", "\\b\\"], ["..", "a"], [".", "."], ["a", "..", ".."], ["//a", "b"], ["\\\\a", "b"], ["\\\\a\\b", "c"], ["\\\\", "a"], ["//", "a"], ["\\\\", "a", "b"], ["/", "/", "a"], ["C:", "a"], ["C:\\", "a"], ["a", "C:b"], ["C:\\a\\b", "..\\..\\.."]]) {
  expr(`P.win32.join(${parts.map((p) => JSON.stringify(p)).join(", ")})`);
}
for (const parts of [["C:\\"], ["C:\\a", "b"], ["C:\\a", "D:\\b"], ["C:\\a", "D:\\b", "c"],["C:\\a\\b", "..", ".."], ["C:\\a\\b", "..\\..\\.."], ["x", "C:\\a", "", "b"], ["C:\\a", "", ""], ["C:\\a\\", "b\\"], ["C:\\a", ".\\b", "..\\c"],
  ["\\\\srv\\sh\\a", "..\\b"], ["\\\\srv\\sh", "x"], ["\\\\srv\\sh\\a", "\\\\other\\sh\\b"], ["\\\\srv\\sh\\a", "C:\\b", "c"], ["C:\\a", "\\\\srv\\sh\\b"], ["C:\\a", "\\b"], ["C:\\a", "/b"], ["\\\\?\\C:\\a", "b"], ["C:\\a", "C:..\\b"], ["c:\\a", "C:b"]]) {
  expr(`P.win32.resolve(${parts.map((p) => JSON.stringify(p)).join(", ")})`);
}
for (const [from, to] of [["C:\\a\\b\\c", "C:\\a\\d"], ["C:\\a", "C:\\a\\b\\c"], ["C:\\a\\b", "C:\\a"], ["C:\\", "C:\\a"], ["C:\\a", "C:\\"], ["C:\\", "C:\\"], ["C:\\aaa\\bbb", "C:\\aaa\\bbbb"], ["C:\\aaa\\bbbb", "C:\\aaa\\bbb"], ["C:\\a\\b", "C:\\a\\b"], ["C:\\a\\b\\", "C:\\a\\b"], ["C:\\a\\\\b", "C:\\a\\c"],
  ["C:\\a\\B", "c:\\a\\b\\c"], ["C:\\a\\b", "D:\\a\\b"], ["C:\\a", "D:\\b"], ["C:\\a\\b\\..\\c", "C:\\a\\c"], ["C:\\foo\\bar", "C:\\foo\\bar\\baz"], ["C:\\foo", "C:\\bar"], ["C:\\", "C:\\a\\b\\c"],
  ["\\\\s\\h\\a", "\\\\s\\h\\b"], ["\\\\s\\h\\a", "\\\\t\\h\\a"], ["\\\\s\\h\\a", "C:\\a"], ["\\a\\b", "\\a\\c"], ["C:\\a", "C:\\ab"], ["C:\\ä", "C:\\Ä"]]) {
  expr(`P.win32.relative(${JSON.stringify(from)}, ${JSON.stringify(to)})`);
}
for (const obj of ["{}", "{ base: 'a' }", "{ root: 'C:\\\\', base: 'a' }", "{ root: 'C:\\\\', dir: 'C:\\\\x', base: 'a' }", "{ dir: 'x', base: 'a' }", "{ dir: 'x' }", "{ dir: 'C:\\\\', root: 'C:\\\\' }", "{ root: 'C:\\\\' }",
  "{ name: 'n', ext: '.e' }", "{ name: 'n' }", "{ ext: '.e' }", "{ name: 'n', ext: 'e' }", "{ base: 'a', name: 'n', ext: '.e' }", "{ dir: 'x', name: 'n', ext: '.e' }", "{ dir: 1 }", "{ base: 1 }",
  "{ root: 0, dir: '', base: 'a' }", "{ dir: null, base: 'a' }", "{ dir: 'a/b', root: 'a' }", "[]", "function () {}", "new String('x')"]) {
  expr(`P.win32.format(${obj})`);
}
for (const v of ["1", "undefined", "null", "{}", "[]", "true", "Symbol()", "10n", "() => 1"]) {
  for (const f of ["normalize", "isAbsolute", "dirname", "basename", "extname", "parse", "join", "resolve", "relative", "format"]) expr(`P.win32.${f}(${v})`);
  expr(`P.win32.join('a', ${v})`);
  expr(`P.win32.resolve('a', ${v})`);
  expr(`P.win32.resolve(${v}, 'C:\\\\a')`);
  expr(`P.win32.relative('C:\\\\a', ${v})`);
  expr(`P.win32.basename('a', ${v})`);
  expr(`P.win32.toNamespacedPath(${v}) === (${v}) || (typeof (${v}) === 'number' && isNaN(${v}))`);
}
expr(`P.win32.basename('a', undefined)`);
expr(`P.win32.join('a', 'b', 3, 'c')`);
expr(`P.win32.resolve(1, 2, 'C:\\\\x')`);
expr(`P.win32.resolve('C:\\\\x', 1)`);

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  globalThis.require = require; // o eval indireto não enxerga o `require` do módulo
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
