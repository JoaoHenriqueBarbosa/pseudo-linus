// URLPattern contra o bun 1.4.2: superfície (construtor, protótipo, descritores, this alheio, subclasse, inspect,
// conversões WebIDL) e os 349 casos do urlpatterntestdata.json do WPT (licença BSD de 3 cláusulas do
// web-platform-tests), com padrão e entradas só; o esperado é o que o bun do oráculo imprime.
const CASES = __CASES__;

function show(value) {
  if (value === undefined) return "undefined";
  return JSON.stringify(value, (key, v) => (v === undefined ? "<undef>" : v));
}

function error(e) {
  return `${e && e.constructor && e.constructor.name}:${e && e.code}:${e && e.message}`;
}

function attempt(fn) {
  try {
    return show(fn());
  } catch (e) {
    return "throws " + error(e);
  }
}

const components = ["protocol", "username", "password", "hostname", "port", "pathname", "search", "hash"];

// Superfície.
console.log(typeof URLPattern, URLPattern.name, URLPattern.length, Object.getOwnPropertyNames(URLPattern).join());
const globalDescriptor = Object.getOwnPropertyDescriptor(globalThis, "URLPattern");
console.log(globalDescriptor.writable, globalDescriptor.enumerable, globalDescriptor.configurable);
console.log(require("node:url").URLPattern === URLPattern);
const proto = URLPattern.prototype;
console.log(Object.getPrototypeOf(proto) === Object.prototype, Object.getPrototypeOf(URLPattern) === Function.prototype);
for (const key of Reflect.ownKeys(proto)) {
  const d = Object.getOwnPropertyDescriptor(proto, key);
  const kind = "value" in d ? `value:${typeof d.value}:${d.writable}` : `get:${typeof d.get}:set:${typeof d.set}`;
  console.log(String(key), kind, d.enumerable, d.configurable);
}
console.log(proto.constructor === URLPattern, Object.prototype.toString.call(proto), proto[Symbol.toStringTag]);
console.log(attempt(() => URLPattern()));
console.log(attempt(() => URLPattern.call({}, "https://a/")));
for (const name of components.concat("hasRegExpGroups")) {
  const getter = Object.getOwnPropertyDescriptor(proto, name).get;
  console.log(name, attempt(() => getter.call({})), attempt(() => getter.call(proto)));
}
console.log(attempt(() => proto.test.call({}, "https://a/")), attempt(() => proto.exec.call(1)));

const p = new URLPattern("https://example.com/:a/:b?");
console.log(Object.prototype.toString.call(p), Object.keys(p).length, Reflect.ownKeys(p).length, p instanceof URLPattern);
console.log(String(p), JSON.stringify(p));
console.log(p);
console.log(require("node:util").inspect(p));
console.log([p]);
console.log({ p });
console.log(Bun.inspect(p));
const result = p.exec("https://example.com/x/y");
console.log(result);
console.log(Object.keys(result).join(), Object.keys(result.pathname).join(), Object.getPrototypeOf(result) === Object.prototype);
console.log(Object.keys(result.pathname.groups).join(), Array.isArray(result.inputs));
console.log(p.exec("https://example.com/x"));
console.log(p.exec("https://example.com/x/y/z"));

class Sub extends URLPattern {
  extra() {
    return this.pathname;
  }
}
const sub = new Sub({ pathname: "/s/*" });
console.log(sub instanceof Sub, sub instanceof URLPattern, sub.extra(), sub.test({ pathname: "/s/1" }), Object.getPrototypeOf(sub) === Sub.prototype);
console.log(Reflect.construct(URLPattern, [{ pathname: "/r" }], Sub) instanceof Sub);

// Conversões WebIDL do primeiro argumento, da base e das opções.
const conversions = [
  [],
  [undefined],
  [null],
  [{}],
  [42, "https://a/"],
  [true, "https://a/"],
  ["https://a/", undefined],
  ["https://a/", null],
  ["https://a/", {}],
  ["/x", "https://a/"],
  ["/x"],
  [{ pathname: "/x" }, "https://a/"],
  [{ pathname: "/x" }, undefined, { ignoreCase: true }],
  ["https://a/", "https://b/", { ignoreCase: 1 }],
  ["https://a/x", { ignoreCase: true }],
  ["https://a/", 5],
  ["https://a/", "https://b/", 5],
  ["https://a/", undefined, undefined],
  [{ pathname: "/x" }, undefined, undefined],
  [Symbol("s")],
  [{ pathname: Symbol("s") }],
  [{ get pathname() { throw new RangeError("boom"); } }],
  [{ pathname: { toString() { return "/t"; } } }],
  [{ pathname: "/\ud800" }],
  ["https://a/\udc00"],
  [{ protocol: "https", port: 443 }],
  [{ baseURL: "https://u:p@h.example:8080/a/b?q#f" }],
  [{ baseURL: "not a url" }],
  [{ hostname: "[::1]" }],
  [{ port: "99999" }],
  [{ protocol: "bad scheme" }],
  [{ pathname: "(" }],
  [{ pathname: "/:a/:a" }],
  [{ pathname: "/(\\d+" }],
  [{ pathname: "/([)" }],
];
for (const args of conversions) {
  console.log(
    attempt(() => {
      const made = new URLPattern(...args);
      return components.map(c => made[c]).concat(made.hasRegExpGroups);
    }),
  );
}

const ip = new URLPattern({ pathname: "/:x" });
const inputs = [[], [undefined], [null], [{}], ["https://a/x"], ["/x", "https://a/"], ["/x"], ["/x", "not a url"], [{ pathname: "/x" }, "https://a/"], [{ pathname: "/x", baseURL: "https://a/" }], [5], [{ pathname: "/y" }, undefined], [{ protocol: "bad scheme" }]];
for (const args of inputs) {
  console.log(attempt(() => ip.test(...args)), attempt(() => ip.exec(...args)));
}

const ci = new URLPattern({ pathname: "/Foo/:id" }, { ignoreCase: true });
console.log(ci.test({ pathname: "/foo/1" }), ci.test({ pathname: "/FOO/1" }), new URLPattern({ pathname: "/Foo" }).test({ pathname: "/foo" }));
const groups = new URLPattern({ pathname: "/(a)/:name/(b)/*" }).exec({ pathname: "/a/n/b/rest" });
console.log(Object.keys(groups.pathname.groups).join(), show(groups.pathname.groups));
console.log(attempt(() => new URLPattern({ pathname: "/(?<inner>x)" }).exec({ pathname: "/x" })));
console.log(attempt(() => new URLPattern({ pathname: "/(?<n>a)(?<m>b)" }).exec({ pathname: "/ab" })));
console.log(attempt(() => new URLPattern({ hostname: "{:sub.}?example.com" }).exec({ hostname: "a.example.com" })));
console.log(attempt(() => new URLPattern({ pathname: "/:été" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/:\u{1F600}" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/(é)" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/:a\\" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/()" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/a()" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/(a)(" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/((?:a))" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/((a))" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/(?a)" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/[a-z]" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/([a-z--b])" }).pathname));
console.log(attempt(() => new URLPattern({ pathname: "/(\\p{L}+)" }).exec({ pathname: "/abc" })));
console.log(attempt(() => new URLPattern({ username: "a b", password: "c@d" }).username));
console.log(attempt(() => new URLPattern({ username: "a b", password: "c@d" }).exec("https://a%20b:c%40d@h/")));

// Os casos do WPT.
let index = 0;
for (const entry of CASES) {
  const line = [`#${index++}`];
  let pattern;
  try {
    pattern = new URLPattern(...entry.pattern);
  } catch (e) {
    line.push("ctor " + error(e));
    console.log(line.join(" | "));
    continue;
  }
  line.push(components.map(c => pattern[c]).join(" ¦ "), String(pattern.hasRegExpGroups));
  const args = entry.inputs ?? [];
  line.push(attempt(() => pattern.test(...args)));
  line.push(attempt(() => pattern.exec(...args)));
  console.log(line.join(" | "));
}
