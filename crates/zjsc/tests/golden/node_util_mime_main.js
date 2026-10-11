// `util.MIMEType` e `util.MIMEParams` contra o bun 1.4.2: a forma das classes (propriedades, descritores, acessores
// nativos), o parser (tipo, subtipo, parâmetros com aspas, barras invertidas, espaços, duplicados), o `toString` e o
// `toJSON`, as operações de `MIMEParams` (`get`, `has`, `set`, `delete`, `entries`, `keys`, `values`, iteração), os
// erros `ERR_INVALID_MIME_SYNTAX`, o `this` alheio, a chamada sem `new`, a subclasse e o `util.inspect`. Roda como
// `/app/main.js`.
const util = require("util");
const { MIMEType, MIMEParams } = util;
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.code, error && error.message];
  }
};
const shape = (name, value) => {
  const descriptors = Object.getOwnPropertyDescriptors(value);
  out(name, "keys", Reflect.ownKeys(value).map(String));
  for (const key of Reflect.ownKeys(value)) {
    const d = descriptors[key];
    out(
      name,
      String(key),
      "data" in d || "value" in d ? ["data", d.writable, d.enumerable, d.configurable, typeof d.value, typeof d.value === "function" ? [d.value.name, d.value.length] : null] : ["accessor", d.enumerable, d.configurable, typeof d.get, typeof d.set, d.get && [d.get.name, d.get.length], d.set && [d.set.name, d.set.length]],
    );
  }
};

// A forma.
shape("MIMEType", MIMEType);
shape("MIMEType.prototype", MIMEType.prototype);
shape("MIMEParams", MIMEParams);
shape("MIMEParams.prototype", MIMEParams.prototype);
out("names", MIMEType.name, MIMEType.length, MIMEParams.name, MIMEParams.length, typeof MIMEType, typeof MIMEParams);
out("tags", Object.prototype.toString.call(MIMEType.prototype), Object.prototype.toString.call(MIMEParams.prototype), Object.prototype.toString.call(new MIMEType("a/b")), Object.prototype.toString.call(new MIMEParams()));
out("proto-chain", Object.getPrototypeOf(MIMEType.prototype) === Object.prototype, Object.getPrototypeOf(MIMEParams.prototype) === Object.prototype, Object.getPrototypeOf(MIMEType) === Function.prototype);
out("exports", Object.getOwnPropertyDescriptor(util, "MIMEType") && Object.keys(Object.getOwnPropertyDescriptor(util, "MIMEType")), MIMEType === util.MIMEType);
out("tostring-fn", String(MIMEType), String(MIMEParams), String(MIMEType.prototype.toString), String(Object.getOwnPropertyDescriptor(MIMEType.prototype, "type").get));
out("same-fn", MIMEType.prototype.toJSON === MIMEType.prototype.toString, MIMEParams.prototype.toJSON === MIMEParams.prototype.toString, MIMEParams.prototype[Symbol.iterator] === MIMEParams.prototype.entries);

// Construção.
out("call", attempt(() => MIMEType("a/b")), attempt(() => MIMEParams()));
out("new-params", attempt(() => new MIMEParams().toString()), attempt(() => new MIMEParams("a=b").toString()), attempt(() => [...new MIMEParams()]));
out("no-args", attempt(() => new MIMEType()), attempt(() => new MIMEType(undefined)), attempt(() => new MIMEType(null).toString()));
out("non-string", attempt(() => new MIMEType({ toString: () => "text/plain" }).toString()), attempt(() => new MIMEType(Symbol("x"))), attempt(() => new MIMEType({ toString() { throw new RangeError("boom"); } })));

// O parser.
const cases = [
  "text/plain",
  "TEXT/PLAIN",
  "  text/plain",
  "text/plain  ",
  "text/plain; charset=utf-8",
  "text/plain;charset=utf-8",
  "text/plain ; charset=utf-8",
  "text/plain;CHARSET=UTF-8",
  "text/plain;a=1;b=2;a=3",
  "text/plain;a=\"x y\"",
  "text/plain;a=\"x\\\"y\"",
  "text/plain;a=\"x\\\\y\"",
  "text/plain;a=\"unterminated",
  "text/plain;a=\"unterminated;b=2",
  "text/plain;a=",
  "text/plain;a=;b=2",
  "text/plain;a",
  "text/plain;a;b=2",
  "text/plain;=b",
  "text/plain;a b=c",
  "text/plain;a=b c",
  "text/plain;a=b\tc",
  "text/plain;a=\"\"",
  "text/plain;a=\"\";b=\"\"",
  "text/plain;;;a=1;;",
  "text/plain; \t\r\n a=1",
  "text/plain;a=1 \t ",
  "text/plain;a=é",
  "text/plain;a=Ā",
  "text/plain;a=\u007f",
  "text/plain;a=\"é\"",
  "text/plain;a=\"line\nbreak\"",
  "text/plain;a=\"\\",
  "text/plain;a=\\x",
  "text/plain;a=\"a\"b",
  "text/plain;a=\"a\"\"b\"",
  "application/vnd.api+json;version=1",
  "a/b;c=d;e=f",
  "!#$%&'*+-.^_`|~/!#$%&'*+-.^_`|~",
  "text",
  "/plain",
  "text/",
  "text/ plain",
  "te xt/plain",
  "text/plain/extra",
  "text/pla in",
  "",
  " ",
  "text/plain;",
  "text/plain;a=1;",
  "texté/plain",
  "text/pléin",
  ";a=1",
  "text/;a=1",
  "/;a=1",
];
for (const input of cases) {
  const result = attempt(() => {
    const mime = new MIMEType(input);
    return [mime.type, mime.subtype, mime.essence, mime.toString(), JSON.stringify(mime), [...mime.params.entries()], mime.params.toString()];
  });
  out("parse", JSON.stringify(input), result);
}

// Os acessores.
const mime = new MIMEType("text/plain;charset=utf-8");
out("getters", mime.type, mime.subtype, mime.essence, mime.params instanceof MIMEParams, mime.params === mime.params);
out("set-type", attempt(() => { mime.type = "IMAGE"; return mime.type; }), attempt(() => { mime.type = ""; }), attempt(() => { mime.type = "a b"; }), attempt(() => { mime.type = "a/b"; }), attempt(() => { mime.type = 5; return mime.type; }), attempt(() => { mime.type = { toString() { throw new RangeError("x"); } }; }));
out("set-subtype", attempt(() => { mime.subtype = "JPEG"; return mime.subtype; }), attempt(() => { mime.subtype = ""; }), attempt(() => { mime.subtype = "a;b"; }), attempt(() => { mime.subtype = undefined; return mime.subtype; }));
out("after-set", mime.toString(), mime.essence);
out("set-readonly", attempt(() => { mime.essence = "x/y"; return mime.essence; }), attempt(() => { "use strict"; mime.params = {}; return 1; }));
out("strict-set", attempt(() => { (function () { "use strict"; mime.essence = "x/y"; })(); }), attempt(() => { (function () { "use strict"; mime.params = null; })(); }));
out("wrong-this", attempt(() => Object.getOwnPropertyDescriptor(MIMEType.prototype, "type").get.call({})), attempt(() => Object.getOwnPropertyDescriptor(MIMEType.prototype, "type").set.call({}, "a")), attempt(() => MIMEType.prototype.toString.call({})), attempt(() => MIMEType.prototype.toString.call(null)), attempt(() => Object.getOwnPropertyDescriptor(MIMEType.prototype, "essence").get.call(1)), attempt(() => Object.getOwnPropertyDescriptor(MIMEType.prototype, "params").get.call("s")));
out("proto-access", attempt(() => MIMEType.prototype.type), attempt(() => MIMEType.prototype.params), attempt(() => MIMEType.prototype.essence));

// MIMEParams.
const params = new MIMEType("text/plain;a=1;b=\"x y\";c=").params;
out("params", params.toString(), [...params], [...params.keys()], [...params.values()], [...params.entries()]);
out("get-has", params.get("a"), params.get("A"), params.get("missing"), params.has("a"), params.has("A"), params.has("c"), params.get(), params.get(undefined), params.has(), params.get(1), params.get({ toString: () => "a" }));
out("get-throws", attempt(() => params.get(Symbol("s"))), attempt(() => params.has({ toString() { throw new RangeError("b"); } })));
params.set("d", "4");
params.set("a", "changed");
params.set("E", "5");
out("after-set", params.toString(), [...params.keys()]);
out("set-invalid", attempt(() => params.set("", "x")), attempt(() => params.set("a b", "x")), attempt(() => params.set("a=b", "x")), attempt(() => params.set("é", "x")), attempt(() => params.set("a", "Ā")), attempt(() => params.set("a", "x\ny")), attempt(() => params.set("a", "x\u007fy")), attempt(() => params.set()), attempt(() => params.set("a")), attempt(() => params.set("x", undefined)), attempt(() => params.set("y", null)), attempt(() => params.set("z", 5)), attempt(() => params.set("z", { toString: () => "obj" })));
out("after-invalid", params.toString());
out("encode", (() => { const p = new MIMEParams(); p.set("a", ""); p.set("b", "tok"); p.set("c", "has space"); p.set("d", 'q"uote'); p.set("e", "back\\slash"); p.set("f", "semi;colon"); p.set("g", "café"); p.set("h", "tab\there"); p.set("i", "!#$%&'*+-.^_`|~"); return p.toString(); })());
out("delete", params.delete("a"), params.delete("missing"), params.delete(), params.toString(), params.delete("D"), params.has("d"));
out("delete-throws", attempt(() => params.delete(Symbol("s"))));
out("iteration", (() => { const seen = []; for (const [k, v] of params) seen.push(k + "=" + v); return seen; })(), Object.prototype.toString.call(params.entries()), Object.prototype.toString.call(params.keys()), Object.prototype.toString.call(params.values()), Object.getPrototypeOf(params.entries()) === Object.getPrototypeOf(new Map().entries()));
const live = new MIMEParams();
const it = live.entries();
live.set("a", "1");
out("live-iterator", it.next(), it.next());
const keysIt = live.keys();
live.set("b", "2");
out("keys-iterator", keysIt.next(), keysIt.next(), keysIt.next());
out("values-iterator", [...live.values()], [...live], Array.from(live), Object.fromEntries(live));
out("spread-order", (() => { const p = new MIMEParams(); p.set("z", "1"); p.set("a", "2"); p.set("z", "3"); return [...p]; })());
out("params-wrong-this", attempt(() => MIMEParams.prototype.get.call({}, "a")), attempt(() => MIMEParams.prototype.has.call(null, "a")), attempt(() => MIMEParams.prototype.set.call(new Map(), "a", "b")), attempt(() => MIMEParams.prototype.delete.call(undefined, "a")), attempt(() => MIMEParams.prototype.toString.call({})), attempt(() => MIMEParams.prototype.entries.call({})), attempt(() => MIMEParams.prototype.keys.call(1)), attempt(() => MIMEParams.prototype.values.call("s")), attempt(() => MIMEParams.prototype[Symbol.iterator].call({})));
out("params-type-this", attempt(() => MIMEParams.prototype.toString.call(new MIMEType("a/b"))), attempt(() => MIMEType.prototype.toString.call(new MIMEParams())));
out("params-json", JSON.stringify(new MIMEType("a/b;c=d").params), JSON.stringify({ p: new MIMEType("a/b;c=\"d e\"") }), JSON.stringify(new MIMEParams()), `${new MIMEType("a/b;c=d")}`, String(new MIMEType("a/b;c=d").params), new MIMEType("a/b;c=d") + "");

// Subclasse e protótipo.
class Sub extends MIMEType {
  extra() {
    return this.essence;
  }
}
const sub = new Sub("text/html;charset=x");
out("subclass", sub instanceof MIMEType, sub.extra(), sub.toString(), Object.getPrototypeOf(sub) === Sub.prototype, sub.params instanceof MIMEParams, Object.prototype.toString.call(sub));
class SubParams extends MIMEParams {}
const subParams = new SubParams();
subParams.set("a", "b");
out("subclass-params", subParams instanceof MIMEParams, subParams.toString(), Object.getPrototypeOf(subParams) === SubParams.prototype);
out("reflect", (() => { const o = Reflect.construct(MIMEType, ["a/b"], Object); return [Object.getPrototypeOf(o) === Object.prototype, attempt(() => o.type)]; })());
out("reflect-params", (() => { const o = Reflect.construct(MIMEParams, [], Array); return [Object.getPrototypeOf(o) === Array.prototype, attempt(() => MIMEParams.prototype.toString.call(o))]; })());
out("own-props", Reflect.ownKeys(new MIMEType("a/b")).map(String), Reflect.ownKeys(new MIMEParams()).map(String), Object.keys(new MIMEType("a/b")), JSON.stringify(Object.getOwnPropertyNames(new MIMEType("a/b;c=d").params)));
out("extensible", Object.isExtensible(new MIMEType("a/b")), attempt(() => { const m = new MIMEType("a/b"); m.custom = 1; return [m.custom, JSON.stringify(m), Object.keys(m)]; }));
out("frozen", attempt(() => { const m = Object.freeze(new MIMEType("a/b")); m.type = "c"; return m.type; }));

// util.inspect e console.log.
out("inspect", util.inspect(new MIMEType("text/plain;charset=utf-8")), util.inspect(new MIMEParams()), util.inspect(new MIMEType("a/b;c=d").params), util.inspect(MIMEType), util.inspect(MIMEType.prototype), util.inspect(MIMEParams.prototype));
console.log(new MIMEType("text/plain;charset=utf-8"));
console.log(new MIMEType("a/b;c=d").params);
console.log([new MIMEType("a/b")], { m: new MIMEParams() });
console.log(MIMEType, MIMEParams);
console.log(MIMEType.prototype, MIMEParams.prototype);
console.log(new MIMEType("a/b;c=d").params.entries());
console.log(util.inspect(new MIMEType("a/b;c=d"), { showHidden: true }), util.inspect(new MIMEType("a/b;c=d"), { depth: 0 }), util.inspect(new MIMEType("a/b;c=d"), { colors: true }));
out("isdeep", util.isDeepStrictEqual(new MIMEType("a/b;c=d"), new MIMEType("a/b;c=d")), util.isDeepStrictEqual(new MIMEType("a/b;c=d"), new MIMEType("a/b;c=e")), util.isDeepStrictEqual(new MIMEParams(), new MIMEParams()));
