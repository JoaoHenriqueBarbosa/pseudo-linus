// `Bun.deepEquals(a, b, strict)` contra o bun 1.4.2: a forma da função e o resultado nos três modos (sem `strict`,
// `strict` falso e `strict` verdadeiro) para primitivos, arrays (buracos e propriedades extras), objetos (chaves
// `undefined`, símbolos, não enumeráveis, protótipos), datas, regexes, coleções, typed arrays, buffers, erros,
// objetos empacotados, funções e ciclos. Roda como `/app/main.js`.
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.code, error && error.message];
  }
};

const descriptor = Object.getOwnPropertyDescriptor(Bun, "deepEquals");
out("descriptor", descriptor.writable, descriptor.enumerable, descriptor.configurable, descriptor.value.name, descriptor.value.length, String(descriptor.value));

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
const symbol = Symbol("s");
const sparse = [1, , 3];
const extra = [1, 2];
extra.name = "x";
const extraOther = [1, 2];
extraOther.name = "y";
const hidden = {};
Object.defineProperty(hidden, "h", { value: 1, enumerable: false });
const getterObject = { get a() { return 1; } };
const fn = () => 1;
const errorWithCause = new Error("x", { cause: 1 });
const errorWithOtherCause = new Error("x", { cause: 2 });
const errorExtra = new Error("x");
errorExtra.code = "E";
const boxedSymbol = Object(symbol);

const cases = [
  ["number", 1, 1], ["number string", 1, "1"], ["nan", NaN, NaN], ["zero", 0, -0], ["bigint", 1n, 1n], ["bigint number", 1n, 1],
  ["string", "a", "a"], ["symbol same", symbol, symbol], ["symbol for", Symbol.for("x"), Symbol.for("x")], ["symbol different", Symbol("a"), Symbol("a")],
  ["null undefined", null, undefined], ["null null", null, null], ["undefined undefined", undefined, undefined],
  ["array", [1, 2], [1, 2]], ["array order", [1, 2], [2, 1]], ["array length", [1], [1, 2]], ["sparse vs undefined", sparse, [1, undefined, 3]],
  ["sparse vs sparse", sparse, [1, , 3]], ["array extra", extra, [1, 2]], ["array extra both", extra, extraOther], ["array vs object", [1], { 0: 1, length: 1 }],
  ["object", { a: 1 }, { a: 1 }], ["object undefined key", { a: 1 }, { a: 1, b: undefined }], ["object only undefined", { a: undefined }, {}],
  ["object key order", { a: 1, b: 2 }, { b: 2, a: 1 }], ["object nested", { a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }], ["object nested diff", { a: [1, { b: 2 }] }, { a: [1, { b: 3 }] }],
  ["object symbol key", { [symbol]: 1 }, { [symbol]: 1 }], ["object symbol key diff", { [symbol]: 1 }, { [symbol]: 2 }], ["object symbol vs none", { [symbol]: 1 }, {}],
  ["non enumerable", hidden, {}], ["getter", getterObject, { a: 1 }], ["null proto", Object.create(null), {}], ["class same", new Point(1), new Point(1)],
  ["class vs plain", new Point(1), { x: 1 }], ["class vs other", new Point(1), new Other(1)],
  ["date", new Date(0), new Date(0)], ["date diff", new Date(0), new Date(1)], ["date invalid", new Date(NaN), new Date(NaN)], ["date vs object", new Date(0), {}],
  ["regexp", /a/g, /a/g], ["regexp flags", /a/g, /a/i], ["regexp source", /a/, /b/], ["regexp lastIndex", Object.assign(/a/g, { lastIndex: 1 }), /a/g],
  ["map", new Map([[1, 2]]), new Map([[1, 2]])], ["map value", new Map([[1, 2]]), new Map([[1, 3]])], ["map object key", new Map([[{ a: 1 }, 1]]), new Map([[{ a: 1 }, 1]])],
  ["map order", new Map([[1, 1], [2, 2]]), new Map([[2, 2], [1, 1]])], ["set", new Set([1]), new Set([1])], ["set diff", new Set([1]), new Set([2])],
  ["set object", new Set([{ a: 1 }]), new Set([{ a: 1 }])], ["set vs map", new Set(), new Map()],
  ["u8", new Uint8Array([1]), new Uint8Array([1])], ["u8 diff", new Uint8Array([1]), new Uint8Array([2])], ["u8 vs i8", new Uint8Array([1]), new Int8Array([1])],
  ["f64 nan", new Float64Array([NaN]), new Float64Array([NaN])], ["f64 zero", new Float64Array([0]), new Float64Array([-0])],
  ["buffer vs u8", Buffer.from([1]), new Uint8Array([1])], ["buffer", Buffer.from("ab"), Buffer.from("ab")],
  ["arraybuffer", new ArrayBuffer(2), new ArrayBuffer(2)], ["arraybuffer diff", new Uint8Array([1]).buffer, new Uint8Array([2]).buffer],
  ["dataview", new DataView(new ArrayBuffer(1)), new DataView(new ArrayBuffer(1))],
  ["string object", new String("a"), "a"], ["string objects", new String("a"), new String("a")], ["number objects", new Number(1), new Number(1)],
  ["number objects diff", new Number(1), new Number(2)], ["boolean objects", new Boolean(true), new Boolean(true)], ["symbol objects", boxedSymbol, Object(symbol)],
  ["function same", fn, fn], ["function diff", () => 1, () => 1], ["class ctor", class A {}, class A {}],
  ["error", new Error("x"), new Error("x")], ["error message", new Error("x"), new Error("y")], ["error type", new Error("x"), new TypeError("x")],
  ["error cause", errorWithCause, errorWithOtherCause], ["error extra", errorExtra, new Error("x")], ["error vs object", new Error("x"), { message: "x" }],
  ["promise", Promise.resolve(1), Promise.resolve(1)], ["weakmap", new WeakMap(), new WeakMap()], ["url", new URL("http://a/"), new URL("http://a/")],
  ["url diff", new URL("http://a/"), new URL("http://b/")], ["headers", new Headers({ a: "1" }), new Headers({ a: "1" })],
  ["arguments", (function () { return arguments; })(1), [1]],
];
for (const [label, left, right] of cases) {
  out("case", label, attempt(() => Bun.deepEquals(left, right)), attempt(() => Bun.deepEquals(left, right, false)), attempt(() => Bun.deepEquals(left, right, true)), attempt(() => Bun.deepEquals(right, left, true)));
}

const cyclicA = { name: "a" };
cyclicA.self = cyclicA;
const cyclicB = { name: "a" };
cyclicB.self = cyclicB;
const cyclicC = { name: "a", self: { name: "a", self: null } };
out("cyclic", Bun.deepEquals(cyclicA, cyclicB), Bun.deepEquals(cyclicA, cyclicB, true), Bun.deepEquals(cyclicA, cyclicC));
const listA = [];
listA.push(listA);
const listB = [];
listB.push(listB);
out("cyclic array", Bun.deepEquals(listA, listB));
out("arity", attempt(() => Bun.deepEquals()), attempt(() => Bun.deepEquals(1)), attempt(() => Bun.deepEquals(1, 1, "yes")), attempt(() => Bun.deepEquals({ a: 1 }, { a: 1, b: undefined }, 1)));
out("skip prototype", Bun.deepEquals(new Point(1), { x: 1 }, true, true), Bun.deepEquals(new Point(1), new Other(1), true, true));
const throwing = { get a() { throw new Error("getter"); } };
out("getter throws", attempt(() => Bun.deepEquals(throwing, { a: 1 })));
const proxy = new Proxy({ a: 1 }, {});
out("proxy", attempt(() => Bun.deepEquals(proxy, { a: 1 })), attempt(() => Bun.deepEquals(proxy, { a: 1 }, true)));
