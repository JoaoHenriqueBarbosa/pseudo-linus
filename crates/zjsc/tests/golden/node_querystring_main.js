// node:querystring contra o bun 1.4.2: superfície, parse, stringify, escape e unescape, opções e erros.
const qs = require("querystring");
const show = (label, f) => {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e) {
    console.log(label, "throws", e.constructor.name, e.code, e.message);
  }
};
console.log(qs === require("node:querystring"), Object.keys(qs).join(","));
console.log(qs.encode === qs.stringify, qs.decode === qs.parse, qs.unescape.name, qs.escape.name, qs.parse.length);
const p = qs.parse("a=1&b=2&a=3&c&d=&=e&f=%20x+y&g=%zz&h=%E2%82%AC");
console.log(Object.getPrototypeOf(p), Object.keys(p), JSON.stringify(p));
show("sep eq", () => qs.parse("a:1;b:2", ";", ":"));
show("multichar", () => qs.parse("a==1&&b==2", "&&", "=="));
show("maxKeys", () => qs.parse("a=1&b=2&c=3", null, null, { maxKeys: 2 }));
show("maxKeys0", () => qs.parse("a=1&b=2&c=3", null, null, { maxKeys: 0 }));
show("decode", () => qs.parse("a=x", null, null, { decodeURIComponent: s => s.toUpperCase() }));
show("decodethrow", () => qs.parse("a=%E0%A4%A", null, null, {}));
show("nonstring", () => qs.parse(42));
show("empty", () => qs.parse(""));
show("plus", () => qs.parse("a+b=c+d"));
show("proto", () => Object.keys(qs.parse("__proto__=1&constructor=2")));
show("s1", () => qs.stringify({ a: 1, b: [1, 2], c: "x y", d: true, e: null, f: undefined, g: NaN, h: 1e21, i: 10n, j: {} }));
show("s2", () => qs.stringify({ a: "€", b: "😀", c: "~!*()'-._" }));
show("s3", () => qs.stringify({ a: 1, b: 2 }, ";", ":"));
show("s4", () => qs.stringify({ a: "x" }, null, null, { encodeURIComponent: s => `<${s}>` }));
show("s5", () => qs.stringify("str"));
show("s6", () => qs.stringify({ "a b": "c" }, "&", "=", { encodeURIComponent: encodeURIComponent }));
show("lone", () => qs.stringify({ a: "\ud800" }));
show("lone2", () => qs.escape("x\udc00"));
show("esc", () => qs.escape("a b&c=d/é"));
show("escobj", () => qs.escape({ toString() { return "q r"; } }));
show("escsym", () => qs.escape(Symbol("x")));
show("unesc", () => qs.unescape("a%20b+c%zz%E2%82"));
show("unescsp", () => qs.unescape("a+b", true));
const buf = qs.unescapeBuffer("a%41+%42%");
console.log(Buffer.isBuffer(buf), buf.length, JSON.stringify(buf.toString()));
console.log(qs.unescapeBuffer("x+y", true).toString());
try {
  qs.stringify({ a: "\ud800" });
} catch (e) {
  console.log(e instanceof URIError, Object.keys(e), e.name, String(e));
}
