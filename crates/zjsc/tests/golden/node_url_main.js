// node:url contra o bun 1.4.2: superfície, url.parse/format/resolve legados, domainToASCII/Unicode, pathToFileURL,
// fileURLToPath(Buffer), urlToHttpOptions e os erros com código.
const url = require("url");
const show = (label, f) => {
  try {
    const v = f();
    console.log(label, typeof v === "string" ? JSON.stringify(v) : v);
  } catch (e) {
    console.log(label, "throws", e.constructor.name, e.name, e.code, JSON.stringify(e.message), Object.keys(e).join(","), e.input === undefined ? "" : String(e.input));
  }
};
console.log(url === require("node:url"), Object.keys(url).join(","));
console.log(url.URL === URL, url.URLSearchParams === URLSearchParams, typeof url.Url, url.parse.name, url.format.name, url.resolve.name);
const parsed = url.parse("https://user:pw@sub.Example.COM:8080/p/a/t/h?query=string&a=1#hash");
console.log(Object.getPrototypeOf(parsed) === url.Url.prototype, Object.keys(parsed).join(","));
console.log(JSON.stringify(parsed));
console.log(JSON.stringify(url.parse("/a/b?x=1&x=2", true)));
console.log(JSON.stringify(url.parse("//host/path", false, true)));
console.log(JSON.stringify(url.parse("mailto:a@b.c")));
console.log(JSON.stringify(url.parse("javascript:alert(1)")));
console.log(JSON.stringify(url.parse("http://ex ample.com/a b<c>")));
console.log(JSON.stringify(url.parse("http://müller.de/ä")));
console.log(JSON.stringify(url.parse("http://[::1]:3000/x")));
console.log(JSON.stringify(url.parse("HTTP://X.COM")));
console.log(JSON.stringify(url.parse("http://a\\b/c\\d")));
show("parse num", () => url.parse(5));
show("parse bad", () => url.parse("http://a%zz.com"));
show("parse port", () => url.parse("http://a.com:99999999999/"));
console.log(url.format(parsed));
console.log(url.format({ protocol: "http", host: "a.com", pathname: "/x y", query: { a: 1, b: [2, 3] } }));
console.log(url.format({ protocol: "file", pathname: "/etc/passwd" }));
console.log(url.format("http://a.com/b?c#d"));
console.log(url.format(new URL("http://u:p@a.com/x?y#z"), { auth: false, search: false, fragment: false, unicode: true }));
show("format bad", () => url.format(5));
for (const [from, to] of [["/one/two/three", "four"], ["http://example.com/", "/one"], ["http://example.com/one", "/two"], ["http://a/b/c/d;p?q", "../../../g"], ["http://a/b/c", "//x/y"], ["mailto:a@b", "c"]]) {
  console.log("resolve", url.resolve(from, to));
}
console.log(JSON.stringify(url.resolveObject("http://a/b/c", "../d?e")));
for (const d of ["español.com", "中文.com", "xn--iñvalid.com", "EXAMPLE.com", "a:b", "1.2.3", "[::1]", "x@y", "", "xn--a"]) {
  console.log("domain", JSON.stringify(d), JSON.stringify(url.domainToASCII(d)), JSON.stringify(url.domainToUnicode(d)));
}
show("domainToASCII()", () => url.domainToASCII());
console.log(url.domainToASCII.name, url.domainToASCII.length, url.domainToUnicode.name);
show("p2f", () => url.pathToFileURL("/tmp/a b#c?d%e").href);
show("p2f dir", () => url.pathToFileURL("/tmp/x/").href);
show("p2f rel", () => url.pathToFileURL("rel/x").href.startsWith("file:///"));
show("p2f win", () => url.pathToFileURL("C:\\a\\b", { windows: true }).href);
show("p2f unc", () => url.pathToFileURL("\\\\server\\share\\f", { windows: true }).href);
show("p2f unc bad", () => url.pathToFileURL("\\\\server", { windows: true }));
show("p2f unc empty", () => url.pathToFileURL("\\\\\\share", { windows: true }));
show("p2f num", () => url.pathToFileURL(5));
show("f2p", () => url.fileURLToPath("file:///tmp/a%20b"));
show("f2p url", () => url.fileURLToPath(new URL("file:///x/y")));
show("f2p http", () => url.fileURLToPath("http://x/"));
show("f2p host", () => url.fileURLToPath("file://host/a"));
show("f2p enc", () => url.fileURLToPath("file:///a%2fb"));
show("f2p num", () => url.fileURLToPath(5));
show("f2p w", () => url.fileURLToPath("file:///C:/a%20b", { windows: true }));
show("f2p w host", () => url.fileURLToPath("file://srv/share/x", { windows: true }));
show("f2p w rel", () => url.fileURLToPath("file:///a/b", { windows: true }));
show("f2p w enc", () => url.fileURLToPath("file:///C:/a%5cb", { windows: true }));
show("f2p p http", () => url.fileURLToPath("http://x/", { windows: false }));
show("f2p p host", () => url.fileURLToPath("file://host/a", { windows: false }));
show("f2p p num", () => url.fileURLToPath(5, { windows: false }));
const b = url.fileURLToPathBuffer("file:///tmp/%E2%82%AC%zz");
console.log(Buffer.isBuffer(b), b.toString("hex"));
show("f2pb http", () => url.fileURLToPathBuffer("http://x/"));
show("f2pb host", () => url.fileURLToPathBuffer("file://h/x"));
show("f2pb num", () => url.fileURLToPathBuffer(5));
show("f2pb w", () => url.fileURLToPathBuffer("file:///C:/x", { windows: true }).toString());
console.log(JSON.stringify(url.urlToHttpOptions(new URL("https://a%20b:c@[::1]:8443/p?q#h"))));
console.log(JSON.stringify(url.urlToHttpOptions(new URL("http://x.com/"))));
// Onde fica o `code` do ERR_MISSING_ARGS: no protótipo no caminho do bun, próprio só no "Not enough arguments" do WebCore.
for (const f of [() => url.domainToUnicode(), () => URL.createObjectURL(), () => Bun.sleepSync(), () => performance.mark()]) {
  try {
    f();
  } catch (e) {
    console.log(e.code, JSON.stringify(e.message), Object.hasOwn(e, "code"), Object.keys(e).join(","));
  }
}
