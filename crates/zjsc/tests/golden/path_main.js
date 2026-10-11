// `require("path")` contra o bun 1.4.2: a forma do módulo (chaves, identidades, nomes e aridades) e o comportamento
// de cada função nos dois sabores (posix e win32), com entradas que não dependem do diretório corrente.
const path = require("path");
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));

out("keys", Object.keys(path).join(","));
out("names", Object.getOwnPropertyNames(path).join(","));
out("identity", path === path.posix, require("node:path") === path, require("path/posix") === path, require("path/win32") === path.win32, require("node:path/posix") === path);
out("cross", path.win32.win32 === path.win32, path.win32.posix === path, path.posix.win32 === path.win32, path._makeLong === path.toNamespacedPath, path.win32._makeLong === path.win32.toNamespacedPath);
out("sep", path.sep, path.delimiter, path.win32.sep, path.win32.delimiter);
out("descriptor", Object.getOwnPropertyDescriptor(path, "sep"), Object.getOwnPropertyDescriptor(path, "join").enumerable, typeof Object.getOwnPropertyDescriptor(path, "join").value);
for (const name of ["resolve", "normalize", "isAbsolute", "join", "relative", "toNamespacedPath", "dirname", "basename", "extname", "format", "parse"]) {
  out("shape", name, path[name].name, path[name].length, String(path[name]), Object.getOwnPropertyNames(path[name]).join(","));
}

const posix = ["", ".", "..", "/", "//", "a", "a/", "a//b", "/a/b/c", "/a/b/../c", "./a/./b", "../a", "a/..", "/a/b/c/", "/a/b.txt", "a.b.c", ".bashrc", "..", "...", "a/.b", "/x/y.tar.gz", "\u00e9/\u00fc.txt", "a\\b"];
const win = ["", ".", "C:", "C:\\", "C:\\a\\b", "C:/a/b", "C:a", "\\\\srv\\sh\\x", "\\\\?\\C:\\a", "\\a", "a\\b\\", "a/b", "C:\\a\\..\\b", "c:\\x.txt", "\\\\srv\\sh", "a:b\\c"];
const cwdWin = process.cwd().split("/").join("\\");
// O win32 resolve o caminho relativo no diretório corrente do processo; o texto dele sai como `CWD` para a saída não
// depender de onde o programa roda.
const namespaced = (flavor, input) => flavor.toNamespacedPath(input).split(cwdWin).join("CWD");
for (const [label, flavor, inputs] of [["posix", path.posix, posix], ["win32", path.win32, win]]) {
  for (const input of inputs) {
    out(label, "case", input, flavor.normalize(input), flavor.isAbsolute(input), flavor.dirname(input), flavor.basename(input), flavor.basename(input, ".txt"), flavor.extname(input), flavor.parse(input), namespaced(flavor, input));
    out(label, "format", flavor.format(flavor.parse(input)));
  }
}

out("join", path.join(), path.join(""), path.join("a", "", "b/", "../c"), path.join("/a", "..", ".."), path.join("..", "a"), path.join("a", "b", "..", "..", ".."), path.join("/", "a"), path.join("a", "/b"));
out("join-win", path.win32.join("C:\\", "a", "..", "b"), path.win32.join("\\\\srv", "sh", "x"), path.win32.join("a", "b"), path.win32.join("C:", "a"), path.win32.join());
out("resolve-abs", path.resolve("/a", "b", "../c"), path.resolve("/a", "/b", "c"), path.resolve("/", ".."), path.resolve("/a/b", "./c/../d"), path.resolve("/a", ""));
out("resolve-win", path.win32.resolve("C:\\a", "b"), path.win32.resolve("C:\\a", "D:\\b"), path.win32.resolve("\\\\srv\\sh\\x", "..", "y"));
out("resolve-cwd", path.resolve("x") === path.join(process.cwd(), "x"), path.resolve() === process.cwd(), path.resolve("") === process.cwd());
for (const [from, to] of [["/a/b/c", "/a/d"], ["/a", "/a"], ["/a/b", "/a/b/c/d"], ["/", "/x"], ["/x", "/"], ["/a/b/c", "/a/b/c"], ["/data/orandea/test/aaa", "/data/orandea/impl/bbb"]]) {
  out("relative", from, to, path.relative(from, to));
}
out("relative-win", path.win32.relative("C:\\a\\b", "C:\\a\\c"), path.win32.relative("C:\\a", "D:\\b"), path.win32.relative("C:\\A\\b", "c:\\a\\b\\x"), path.win32.relative("\\\\srv\\sh\\a", "\\\\srv\\sh\\b"));
out("basename-suffix", path.basename("/a/b.txt", "b.txt"), path.basename("/a/b.txt", ".txt"), path.basename("/a/b.txt", "txt"), path.basename("/a/b.txt/", ".txt"), path.basename("/a/b.txt", "x.txt"), path.basename("aaa", "aaa"));
out("format", path.format({ dir: "/a", base: "b.txt" }), path.format({ root: "/", base: "b" }), path.format({ name: "n", ext: "e" }), path.format({ name: "n", ext: ".e" }), path.format({ dir: "/", root: "/", base: "x" }), path.format({}), path.win32.format({ dir: "C:\\a", base: "b" }));

const attempt = (label, fn) => {
  try {
    out(label, fn());
  } catch (error) {
    out(label, "threw", error.name, error.code, error.message, error instanceof TypeError);
  }
};
for (const bad of [1, null, undefined, {}, [], true, Symbol.iterator, 10n]) {
  attempt("join-bad", () => path.join("a", bad));
  attempt("normalize-bad", () => path.normalize(bad));
  attempt("isAbsolute-bad", () => path.isAbsolute(bad));
  attempt("dirname-bad", () => path.dirname(bad));
  attempt("basename-bad", () => path.basename(bad));
  attempt("basename-ext-bad", () => path.basename("a", bad));
  attempt("extname-bad", () => path.extname(bad));
  attempt("parse-bad", () => path.parse(bad));
  attempt("relative-bad-from", () => path.relative(bad, "/a"));
  attempt("relative-bad-to", () => path.relative("/a", bad));
  attempt("resolve-bad", () => path.resolve("/a", bad));
  attempt("format-bad", () => path.format(bad));
  attempt("namespaced-bad", () => path.toNamespacedPath(bad));
}
attempt("win32-join-bad", () => path.win32.join(1));
attempt("win32-format-bad", () => path.win32.format("x"));
attempt("format-fn", () => path.format(() => 1));

const lone = "\ud800";
out("surrogate", path.join("a", lone, "b"), path.basename("/x/" + lone + ".txt").length, path.extname("a." + lone).length, path.normalize("/a/" + lone + "/../b"));
out("tostring", Object.prototype.toString.call(path), Object.keys(path.win32).length, Object.getPrototypeOf(path) === Object.prototype);
