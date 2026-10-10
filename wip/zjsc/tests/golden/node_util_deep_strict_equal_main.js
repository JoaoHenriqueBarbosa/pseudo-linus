// `util.isDeepStrictEqual(a, b, skipPrototype)` contra o bun 1.4.2, a parte dos objetos comuns: a forma da função, a
// regra do `constructor` e do `[[Prototype]]` do `objectComparisonStart` do Node, as etiquetas de `Object.prototype.toString`,
// as propriedades próprias enumeráveis (texto e símbolo, getters, ocultas, herdadas), os arrays com buracos e chaves
// extras, os ciclos e o terceiro argumento (`kStrictWithoutPrototypes`). Os objetos especiais (coleções, datas, typed
// arrays, erros, empacotados) ficam no golden seguinte. Roda como `/app/main.js`.
const util = require("util");
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.code, error && error.message];
  }
};
const eq = (name, a, b, skip) => out(name, attempt(() => util.isDeepStrictEqual(a, b, skip)));
const both = (name, a, b) => {
  out(name, attempt(() => util.isDeepStrictEqual(a, b)), attempt(() => util.isDeepStrictEqual(b, a)), attempt(() => util.isDeepStrictEqual(a, b, true)));
};

const descriptor = Object.getOwnPropertyDescriptor(util, "isDeepStrictEqual");
out("descriptor", descriptor.writable, descriptor.enumerable, descriptor.configurable, descriptor.value.name, descriptor.value.length);
out("arity", attempt(() => util.isDeepStrictEqual()), attempt(() => util.isDeepStrictEqual(1)));

// Primitivos.
both("same-number", 1, 1);
both("nan", NaN, NaN);
both("zeros", 0, -0);
both("string-number", "1", 1);
both("null-undefined", null, undefined);
both("bigint", 1n, 1n);
both("object-primitive", {}, 1);
const symbol = Symbol("s");
both("symbol-same", symbol, symbol);
both("symbol-other", Symbol("s"), Symbol("s"));

// Objetos simples.
both("plain-equal", { a: 1, b: { c: [1, 2, 3] } }, { a: 1, b: { c: [1, 2, 3] } });
both("plain-order", { a: 1, b: 2 }, { b: 2, a: 1 });
both("plain-extra", { a: 1 }, { a: 1, b: 2 });
both("plain-undefined", { a: undefined }, {});
both("plain-undefined-both", { a: undefined }, { b: undefined });
both("plain-value", { a: 1 }, { a: 2 });
both("empty", {}, {});

// Protótipos e construtores.
class Point {
  constructor(x) {
    this.x = x;
  }
}
class Other {
  constructor(x) {
    this.x = x;
  }
}
class Child extends Point {}
both("class-same", new Point(1), new Point(1));
both("class-other", new Point(1), new Other(1));
both("class-plain", new Point(1), { x: 1 });
both("class-child", new Point(1), new Child(1));
both("null-proto", Object.create(null), {});
both("null-proto-both", Object.assign(Object.create(null), { a: 1 }), Object.assign(Object.create(null), { a: 1 }));
both("null-proto-props", Object.assign(Object.create(null), { a: 1 }), { a: 1 });
const protoA = { kind: "a" };
const protoB = { kind: "a" };
both("object-create-same", Object.create(protoA), Object.create(protoA));
both("object-create-other", Object.create(protoA), Object.create(protoB));
both("object-create-proto-null", Object.create(Object.create(null)), Object.create(Object.create(null)));
both("set-prototype", Object.setPrototypeOf({ a: 1 }, protoA), { a: 1 });
function Legacy(x) {
  this.x = x;
}
function LegacyOther(x) {
  this.x = x;
}
both("function-ctor-same", new Legacy(1), new Legacy(1));
both("function-ctor-other", new Legacy(1), new LegacyOther(1));
const ownConstructor = { constructor: Point, x: 1 };
both("own-constructor", ownConstructor, new Point(1));
both("own-constructor-plain", { constructor: Object, x: 1 }, { x: 1 });
const shadow = Object.create(Point.prototype);
shadow.x = 1;
Object.defineProperty(shadow, "constructor", { value: Other, enumerable: false });
both("shadow-constructor", shadow, new Point(1));
both("constructor-undefined", Object.setPrototypeOf({ x: 1 }, { constructor: undefined }), Object.setPrototypeOf({ x: 1 }, { constructor: undefined }));
both("constructor-undefined-other", Object.setPrototypeOf({ x: 1 }, { constructor: undefined }), Object.setPrototypeOf({ x: 1 }, {}));
const getterProto = {
  get constructor() {
    return Point;
  },
};
both("getter-constructor", Object.setPrototypeOf({ x: 1 }, getterProto), Object.setPrototypeOf({ x: 1 }, getterProto));
both("getter-constructor-other", Object.setPrototypeOf({ x: 1 }, getterProto), new Point(1));
const throwingProto = {
  get constructor() {
    throw new RangeError("constructor boom");
  },
};
both("constructor-throws", Object.setPrototypeOf({}, throwingProto), {});
both("constructor-throws-right", {}, Object.setPrototypeOf({}, throwingProto));
// Construtores conhecidos de outros intrínsecos pela cadeia.
for (const ctor of [Object, Array, Function, RegExp, Promise, String, Symbol, BigInt, Boolean, Number, Date, Error, Map, Set, WeakMap, WeakSet, ArrayBuffer, DataView, Int8Array, Uint8Array, Float64Array, BigInt64Array, Buffer]) {
  const left = Object.setPrototypeOf({ x: 1 }, { constructor: ctor });
  const right = Object.setPrototypeOf({ x: 1 }, { constructor: ctor });
  both("known-" + ctor.name, left, right);
  both("known-other-" + ctor.name, left, Object.setPrototypeOf({ x: 1 }, { constructor: Point }));
}
for (const ctor of [SharedArrayBuffer, Proxy, Math.max, class Foo {}, function () {}, () => {}, (function* () {}).constructor, (async function () {}).constructor, (async function* () {}).constructor, TypeError, WeakRef, FinalizationRegistry, Intl.Collator]) {
  const left = Object.setPrototypeOf({ x: 1 }, { constructor: ctor });
  const right = Object.setPrototypeOf({ x: 1 }, { constructor: ctor });
  both("other-" + (ctor && ctor.name), left, right);
}

// Etiquetas de `Object.prototype.toString`.
const tagged = (tag) => ({ x: 1, [Symbol.toStringTag]: tag });
both("tag-same", tagged("A"), tagged("A"));
both("tag-other", tagged("A"), tagged("B"));
class Tag1 {
  get [Symbol.toStringTag]() {
    return "T";
  }
}
class Tag2 {
  get [Symbol.toStringTag]() {
    return "T";
  }
}
both("tag-class-getter", new Tag1(), new Tag1());
both("tag-class-other", new Tag1(), new Tag2());
const protoTag = (tag) => Object.setPrototypeOf({ x: 1 }, { [Symbol.toStringTag]: tag });
both("tag-inherited", protoTag("A"), protoTag("A"));
both("tag-inherited-other", protoTag("A"), protoTag("B"));
const tagThrows = Object.setPrototypeOf({}, {
  get [Symbol.toStringTag]() {
    throw new RangeError("tag boom");
  },
});
both("tag-throws", tagThrows, {});
both("tag-number", { [Symbol.toStringTag]: 1 }, { [Symbol.toStringTag]: 2 });

// Propriedades: símbolos, ocultas, herdadas e getters.
const sym = Symbol("k");
both("symbol-key", { [sym]: 1 }, { [sym]: 1 });
both("symbol-key-value", { [sym]: 1 }, { [sym]: 2 });
both("symbol-key-missing", { [sym]: 1 }, {});
both("symbol-key-other", { [sym]: 1 }, { [Symbol("k")]: 1 });
const hidden = { a: 1 };
Object.defineProperty(hidden, "h", { value: 1, enumerable: false });
both("hidden", hidden, { a: 1 });
const hiddenOther = { a: 1 };
Object.defineProperty(hiddenOther, "h", { value: 2, enumerable: false });
both("hidden-differs", hidden, hiddenOther);
const inherited = Object.create({ inherited: 1 });
inherited.a = 1;
const plainA = { a: 1 };
both("inherited-vs-plain", inherited, plainA);
const sameProto = { inherited: 1 };
const inheritedA = Object.create(sameProto);
inheritedA.a = 1;
const inheritedB = Object.create(sameProto);
inheritedB.a = 1;
both("inherited-same-proto", inheritedA, inheritedB);
const inheritedC = Object.create(sameProto);
inheritedC.a = 1;
inheritedC.inherited = 1;
both("inherited-own-vs-proto", inheritedA, inheritedC);
let reads = 0;
const getterA = {
  get a() {
    reads++;
    return 1;
  },
};
const getterB = { a: 1 };
both("getter-vs-data", getterA, getterB);
out("getter-reads", reads);
both("getter-both", { get a() { return 1; } }, { get a() { return 1; } });
both("accessor-throws", { get a() { throw new SyntaxError("getter boom"); } }, { a: 1 });
both("accessor-throws-right", { a: 1 }, { get a() { throw new SyntaxError("getter boom"); } });
const removing = {
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
};
both("getter-deletes", removing, { a: 1, b: 2 });
both("index-keys", { 0: "a", 1: "b" }, { 0: "a", 1: "b" });
both("index-keys-value", { 0: "a", 1: "b" }, { 0: "a", 1: "c" });
both("index-vs-string", { 0: "a" }, { "0": "a" });
both("different-structure-same-keys", Object.assign({}, { a: 1, b: 2 }), (() => { const o = { b: 2 }; o.a = 1; return o; })());
const dictA = {};
const dictB = {};
for (let i = 0; i < 200; i++) {
  dictA["k" + i] = i;
  dictB["k" + (199 - i)] = 199 - i;
}
both("dictionary-objects", dictA, dictB);
delete dictB.k5;
both("dictionary-missing", dictA, dictB);
const withDeleted = { a: 1, b: 2, c: 3 };
delete withDeleted.b;
both("after-delete", withDeleted, { a: 1, c: 3 });

// Arrays.
both("array-equal", [1, 2, 3], [1, 2, 3]);
both("array-length", [1, 2], [1, 2, 3]);
both("array-hole", [1, , 3], [1, undefined, 3]);
both("array-hole-both", [1, , 3], [1, , 3]);
both("array-hole-trailing", [1, 2, ,], [1, 2]);
const arrExtra = [1, 2];
arrExtra.name = "x";
both("array-extra-string", arrExtra, [1, 2]);
const arrExtraB = [1, 2];
arrExtraB.name = "x";
both("array-extra-same", arrExtra, arrExtraB);
arrExtraB.name = "y";
both("array-extra-value", arrExtra, arrExtraB);
const arrSym = [1, 2];
arrSym[sym] = 1;
both("array-symbol", arrSym, [1, 2]);
const arrSymB = [1, 2];
arrSymB[sym] = 1;
both("array-symbol-same", arrSym, arrSymB);
const arrHidden = [1, 2];
Object.defineProperty(arrHidden, "h", { value: 1, enumerable: false });
both("array-hidden", arrHidden, [1, 2]);
both("array-vs-object", [1], { 0: 1, length: 1 });
class MyArray extends Array {}
both("array-subclass", MyArray.from([1, 2]), [1, 2]);
both("array-subclass-same", MyArray.from([1, 2]), MyArray.from([1, 2]));
both("array-nested", [[1, [2]], { a: [3] }], [[1, [2]], { a: [3] }]);
both("array-proto-null", Object.setPrototypeOf([1], null), [1]);
const arrGetter = [1, 2];
Object.defineProperty(arrGetter, "0", { get() { return 1; }, enumerable: true });
both("array-accessor", arrGetter, [1, 2]);

// Ciclos.
const cycleA = { name: "c" };
cycleA.self = cycleA;
const cycleB = { name: "c" };
cycleB.self = cycleB;
both("cycle-same-shape", cycleA, cycleB);
const cycleC = { name: "c", self: { name: "c" } };
cycleC.self.self = cycleC;
both("cycle-longer", cycleA, cycleC);
const arrCycleA = [1];
arrCycleA.push(arrCycleA);
const arrCycleB = [1];
arrCycleB.push(arrCycleB);
both("array-cycle", arrCycleA, arrCycleB);
const crossA = { x: {} };
crossA.x.back = crossA;
const crossB = { x: {} };
crossB.x.back = crossB.x;
both("cycle-cross", crossA, crossB);

// O terceiro argumento: só o `kStrictWithoutPrototypes` (as etiquetas seguem valendo).
eq("skip-class", new Point(1), new Other(1), true);
eq("skip-class-falsy", new Point(1), new Other(1), 0);
eq("skip-class-truthy", new Point(1), new Other(1), "yes");
eq("skip-null-proto", Object.create(null), {}, true);
eq("skip-tag", tagged("A"), tagged("B"), true);
eq("skip-arrays", MyArray.from([1]), [1], true);
eq("skip-throwing-getter-arg", new Point(1), new Other(1), { valueOf() { throw new Error("arg boom"); } });
eq("skip-throwing-toboolean", new Point(1), new Other(1), { [Symbol.toPrimitive]() { throw new Error("arg boom"); } });

// Proxies e objetos exóticos que o ramo comum alcança.
const target = { a: 1 };
both("proxy-same-target", new Proxy(target, {}), new Proxy(target, {}));
both("proxy-vs-target", new Proxy(target, {}), target);
both("proxy-equal-targets", new Proxy({ a: 1 }, {}), new Proxy({ a: 1 }, {}));
const traps = [];
const spy = new Proxy({ a: 1 }, {
  getOwnPropertyDescriptor(t, k) {
    traps.push("gopd:" + String(k));
    return Reflect.getOwnPropertyDescriptor(t, k);
  },
  ownKeys(t) {
    traps.push("ownKeys");
    return Reflect.ownKeys(t);
  },
  get(t, k, r) {
    traps.push("get:" + String(k));
    return Reflect.get(t, k, r);
  },
  getPrototypeOf(t) {
    traps.push("getPrototypeOf");
    return Reflect.getPrototypeOf(t);
  },
});
both("proxy-spy", spy, { a: 1 });
out("proxy-traps", traps);
const args = (function () { return arguments; })(1, 2);
const args2 = (function () { return arguments; })(1, 2);
both("arguments", args, args2);
both("arguments-vs-array", args, [1, 2]);
both("module-namespace-like", Object.freeze({ a: 1 }), { a: 1 });
both("frozen", Object.freeze({ a: 1 }), Object.seal({ a: 1 }));
both("globalThis", globalThis, globalThis);
both("math-json", Math, JSON);
both("object-prototype", Object.prototype, Object.prototype);
both("object-prototype-vs-array-prototype", Object.prototype, Array.prototype);
