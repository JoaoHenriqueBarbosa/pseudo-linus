// `util.isDeepStrictEqual` contra o bun 1.4.2, a parte dos objetos especiais: `Set` e `Map` (busca linear, chaves
// objeto, propriedades próprias), `ArrayBuffer`, `SharedArrayBuffer`, `DataView`, `Date` (inválidas iguais), `RegExp`
// (`lastIndex`), typed arrays e `Buffer` (bytes, chaves não-índice, ponto flutuante, NaN), invólucros `String`, `Number`,
// `Boolean`, `Symbol` e `BigInt`, `Error`, `WeakMap`, `WeakSet`, `Promise`, funções e o terceiro argumento. Roda como
// `/app/main.js`.
const util = require("util");
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.code, error && error.message];
  }
};
const both = (name, a, b) => {
  out(name, attempt(() => util.isDeepStrictEqual(a, b)), attempt(() => util.isDeepStrictEqual(b, a)), attempt(() => util.isDeepStrictEqual(a, b, true)));
};
const sym = Symbol("k");

// Set.
both("set-equal", new Set([1, 2, 3]), new Set([3, 2, 1]));
both("set-size", new Set([1, 2]), new Set([1, 2, 3]));
both("set-value", new Set([1, 2]), new Set([1, 3]));
both("set-objects", new Set([{ a: 1 }, { b: 2 }]), new Set([{ b: 2 }, { a: 1 }]));
both("set-objects-differ", new Set([{ a: 1 }]), new Set([{ a: 2 }]));
both("set-nested", new Set([new Set([1]), [1, 2]]), new Set([[1, 2], new Set([1])]));
both("set-nan", new Set([NaN]), new Set([NaN]));
both("set-zeros", new Set([0]), new Set([-0]));
both("set-vs-map", new Set([1]), new Map([[1, 1]]));
both("set-vs-array", new Set([1]), [1]);
const setProp = new Set([1]);
setProp.extra = 1;
both("set-extra", setProp, new Set([1]));
const setPropB = new Set([1]);
setPropB.extra = 1;
both("set-extra-same", setProp, setPropB);
setPropB.extra = 2;
both("set-extra-differs", setProp, setPropB);
class MySet extends Set {}
both("set-subclass", new MySet([1]), new Set([1]));
both("set-subclass-same", new MySet([1]), new MySet([1]));
const setCycleA = new Set();
setCycleA.add(setCycleA);
const setCycleB = new Set();
setCycleB.add(setCycleB);
both("set-cycle", setCycleA, setCycleB);

// Map.
both("map-equal", new Map([[1, "a"], [2, "b"]]), new Map([[2, "b"], [1, "a"]]));
both("map-value", new Map([[1, "a"]]), new Map([[1, "b"]]));
both("map-key", new Map([[1, "a"]]), new Map([[2, "a"]]));
both("map-size", new Map([[1, "a"]]), new Map());
both("map-object-keys", new Map([[{ k: 1 }, "a"]]), new Map([[{ k: 1 }, "a"]]));
both("map-object-keys-differ", new Map([[{ k: 1 }, "a"]]), new Map([[{ k: 2 }, "a"]]));
both("map-undefined-value", new Map([[1, undefined]]), new Map([[1, undefined]]));
both("map-undefined-vs-missing", new Map([[1, undefined]]), new Map([[2, undefined]]));
both("map-nested", new Map([["a", new Map([["b", [1, 2]]])]]), new Map([["a", new Map([["b", [1, 2]]])]]));
const mapProp = new Map([[1, 1]]);
mapProp[sym] = 1;
both("map-symbol-extra", mapProp, new Map([[1, 1]]));
class MyMap extends Map {}
both("map-subclass", new MyMap([[1, 1]]), new Map([[1, 1]]));
const mapCycleA = new Map();
mapCycleA.set("self", mapCycleA);
const mapCycleB = new Map();
mapCycleB.set("self", mapCycleB);
both("map-cycle", mapCycleA, mapCycleB);

// ArrayBuffer e SharedArrayBuffer.
const bytes = (...values) => new Uint8Array(values).buffer;
both("ab-equal", bytes(1, 2, 3), bytes(1, 2, 3));
both("ab-bytes", bytes(1, 2, 3), bytes(1, 2, 4));
both("ab-length", bytes(1, 2), bytes(1, 2, 3));
both("ab-empty", new ArrayBuffer(0), new ArrayBuffer(0));
const abProp = bytes(1);
abProp.extra = 1;
both("ab-extra", abProp, bytes(1));
const abPropB = bytes(1);
abPropB.extra = 1;
both("ab-extra-same", abProp, abPropB);
both("ab-vs-shared", new ArrayBuffer(2), new SharedArrayBuffer(2));
both("shared-equal", new SharedArrayBuffer(2), new SharedArrayBuffer(2));
const detached = new ArrayBuffer(4);
structuredClone(detached, { transfer: [detached] });
both("ab-detached", detached, new ArrayBuffer(4));
both("ab-detached-length", detached, new ArrayBuffer(0));
both("ab-resizable", new ArrayBuffer(2, { maxByteLength: 8 }), new ArrayBuffer(2, { maxByteLength: 16 }));

// DataView.
both("dv-equal", new DataView(bytes(1, 2, 3)), new DataView(bytes(1, 2, 3)));
both("dv-bytes", new DataView(bytes(1, 2, 3)), new DataView(bytes(1, 2, 4)));
both("dv-offset", new DataView(bytes(9, 1, 2), 1), new DataView(bytes(1, 2)));
both("dv-length", new DataView(bytes(1, 2, 3), 0, 2), new DataView(bytes(1, 2, 3)));
const dvProp = new DataView(bytes(1));
dvProp.extra = 1;
both("dv-extra", dvProp, new DataView(bytes(1)));
both("dv-index-key", Object.assign(new DataView(bytes(1)), { 5: 1 }), new DataView(bytes(1)));
both("dv-vs-ab", new DataView(bytes(1)), bytes(1));
both("dv-vs-u8", new DataView(bytes(1)), new Uint8Array([1]));
const dvDetachedBuffer = new ArrayBuffer(4);
const dvDetached = new DataView(dvDetachedBuffer);
structuredClone(dvDetachedBuffer, { transfer: [dvDetachedBuffer] });
both("dv-detached", dvDetached, new DataView(new ArrayBuffer(4)));

// Date.
both("date-equal", new Date(5), new Date(5));
both("date-differ", new Date(5), new Date(6));
both("date-invalid", new Date(NaN), new Date(NaN));
both("date-invalid-vs-valid", new Date(NaN), new Date(0));
const dateProp = new Date(5);
dateProp.extra = 1;
both("date-extra", dateProp, new Date(5));
class MyDate extends Date {}
both("date-subclass", new MyDate(5), new Date(5));
both("date-vs-object", new Date(5), {});

// RegExp.
both("re-equal", /a/g, /a/g);
both("re-flags", /a/g, /a/i);
both("re-source", /a/, /b/);
const reUsed = /a/g;
reUsed.lastIndex = 3;
both("re-last-index", reUsed, /a/g);
const reUsedB = /a/g;
reUsedB.lastIndex = 3;
both("re-last-index-same", reUsed, reUsedB);
const reProp = /a/;
reProp.extra = 1;
both("re-extra", reProp, /a/);
both("re-vs-object", /a/, {});

// Typed arrays e Buffer.
both("u8-equal", new Uint8Array([1, 2]), new Uint8Array([1, 2]));
both("u8-differ", new Uint8Array([1, 2]), new Uint8Array([1, 3]));
both("u8-vs-i8", new Uint8Array([1]), new Int8Array([1]));
both("u8-vs-clamped", new Uint8Array([1]), new Uint8ClampedArray([1]));
both("u8-length", new Uint8Array([1]), new Uint8Array([1, 2]));
both("u8-empty", new Uint8Array(0), new Uint8Array(0));
const u8Prop = new Uint8Array([1]);
u8Prop.extra = 1;
both("u8-extra", u8Prop, new Uint8Array([1]));
const u8PropB = new Uint8Array([1]);
u8PropB.extra = 1;
both("u8-extra-same", u8Prop, u8PropB);
u8PropB.extra = 2;
both("u8-extra-differs", u8Prop, u8PropB);
const u8Sym = new Uint8Array([1]);
u8Sym[sym] = 1;
both("u8-symbol", u8Sym, new Uint8Array([1]));
both("u8-empty-extra", Object.assign(new Uint8Array(0), { extra: 1 }), new Uint8Array(0));
const shared = new ArrayBuffer(4);
both("u8-same-buffer", new Uint8Array(shared), new Uint8Array(shared));
both("u8-same-buffer-extra", Object.assign(new Uint8Array(shared), { a: 1 }), new Uint8Array(shared));
both("u8-subarray", new Uint8Array([1, 2, 3]).subarray(1), new Uint8Array([2, 3]));
both("f32-zeros", new Float32Array([0]), new Float32Array([-0]));
both("f32-nan", new Float32Array([NaN]), new Float32Array([NaN]));
both("f64-zeros", new Float64Array([0]), new Float64Array([-0]));
both("f64-nan", new Float64Array([NaN]), new Float64Array([NaN]));
both("f16-zeros", new Float16Array([0]), new Float16Array([-0]));
both("f64-equal", new Float64Array([1.5, 2.5]), new Float64Array([1.5, 2.5]));
both("bigint64", new BigInt64Array([1n]), new BigInt64Array([1n]));
both("bigint64-differ", new BigInt64Array([1n]), new BigInt64Array([2n]));
both("biguint64", new BigUint64Array([1n]), new BigUint64Array([1n]));
both("i16-equal", new Int16Array([1, -2]), new Int16Array([1, -2]));
both("u32-equal", new Uint32Array([1, 2]), new Uint32Array([1, 2]));
both("buffer-equal", Buffer.from("abc"), Buffer.from("abc"));
both("buffer-vs-u8", Buffer.from("abc"), new Uint8Array([97, 98, 99]));
both("buffer-differ", Buffer.from("abc"), Buffer.from("abd"));
class MyU8 extends Uint8Array {}
both("u8-subclass", new MyU8([1]), new Uint8Array([1]));
both("u8-subclass-same", new MyU8([1]), new MyU8([1]));
const detachedBuffer = new ArrayBuffer(4);
const detachedView = new Uint8Array(detachedBuffer);
structuredClone(detachedBuffer, { transfer: [detachedBuffer] });
both("u8-detached", detachedView, new Uint8Array(0));
both("u8-detached-both", detachedView, detachedView);
const growable = new ArrayBuffer(2, { maxByteLength: 8 });
both("u8-length-tracking", new Uint8Array(growable), new Uint8Array(2));

// Invólucros.
both("string-obj", new String("a"), new String("a"));
both("string-obj-differ", new String("a"), new String("b"));
both("string-obj-vs-prim", new String("a"), "a");
both("string-obj-extra", Object.assign(new String("a"), { x: 1 }), new String("a"));
both("string-obj-extra-same", Object.assign(new String("a"), { x: 1 }), Object.assign(new String("a"), { x: 1 }));
both("string-obj-index", Object.assign(new String("a"), { 5: 1 }), new String("a"));
class MyString extends String {}
both("string-subclass", new MyString("a"), new String("a"));
both("string-subclass-same", new MyString("a"), new MyString("a"));
both("number-obj", new Number(1), new Number(1));
both("number-obj-differ", new Number(1), new Number(2));
both("number-obj-zeros", new Number(0), new Number(-0));
both("number-obj-nan", new Number(NaN), new Number(NaN));
both("number-obj-vs-boolean", new Number(1), new Boolean(true));
both("number-obj-extra", Object.assign(new Number(1), { x: 1 }), new Number(1));
both("boolean-obj", new Boolean(true), new Boolean(true));
both("boolean-obj-differ", new Boolean(true), new Boolean(false));
both("symbol-obj", Object(sym), Object(sym));
both("symbol-obj-other", Object(Symbol("a")), Object(Symbol("a")));
both("bigint-obj", Object(1n), Object(1n));
both("bigint-obj-differ", Object(1n), Object(2n));
both("symbol-vs-bigint-obj", Object(1n), Object(sym));
both("bigint-obj-extra", Object.assign(Object(1n), { x: 1 }), Object(1n));

// Erros.
both("error-equal", new Error("a"), new Error("a"));
both("error-message", new Error("a"), new Error("b"));
both("error-type", new Error("a"), new TypeError("a"));
both("error-name", Object.assign(new Error("a"), { name: "X" }), new Error("a"));
both("error-cause", new Error("a", { cause: 1 }), new Error("a", { cause: 1 }));
both("error-cause-differs", new Error("a", { cause: 1 }), new Error("a", { cause: 2 }));
both("error-cause-missing", new Error("a", { cause: undefined }), new Error("a"));
both("error-extra", Object.assign(new Error("a"), { code: 1 }), new Error("a"));
both("error-extra-same", Object.assign(new Error("a"), { code: 1 }), Object.assign(new Error("a"), { code: 1 }));
class MyError extends Error {}
both("error-subclass", new MyError("a"), new Error("a"));
both("error-subclass-same", new MyError("a"), new MyError("a"));
both("error-aggregate", new AggregateError([1], "a"), new AggregateError([1], "a"));
both("error-aggregate-differ", new AggregateError([1], "a"), new AggregateError([2], "a"));
both("error-vs-object", new Error("a"), { message: "a", name: "Error" });

// Weak*, Promise, funções e globais.
const weakKey = {};
both("weakmap", new WeakMap(), new WeakMap());
both("weakmap-same", new WeakMap([[weakKey, 1]]), new WeakMap([[weakKey, 1]]));
both("weakset", new WeakSet(), new WeakSet());
both("promise", Promise.resolve(1), Promise.resolve(1));
const wm = new WeakMap();
both("weakmap-identity", wm, wm);
const pr = Promise.resolve(1);
both("promise-identity", pr, pr);
both("weakmap-vs-object", new WeakMap(), {});
both("promise-vs-object", Promise.resolve(1), {});
const fn = () => 1;
both("function-identity", fn, fn);
both("function-copy", () => 1, () => 1);
both("function-props", Object.assign(() => 1, { a: 1 }), Object.assign(() => 1, { a: 1 }));
both("class-copy", class A {}, class A {});
both("global-this", globalThis, {});
both("generator", (function* () {})(), (function* () {})());
both("map-iterator", new Map([[1, 1]]).entries(), new Map([[1, 1]]).entries());
both("url", new URL("http://a/"), new URL("http://a/"));
both("url-differs", new URL("http://a/"), new URL("http://b/"));
both("url-search-params", new URLSearchParams("a=1"), new URLSearchParams("a=1"));
both("headers", new Headers({ a: "1" }), new Headers({ a: "1" }));

// O terceiro argumento nos especiais.
out("skip-set", attempt(() => util.isDeepStrictEqual(new MySet([1]), new Set([1]), true)));
out("skip-map", attempt(() => util.isDeepStrictEqual(new MyMap([[1, 1]]), new Map([[1, 1]]), true)));
out("skip-date", attempt(() => util.isDeepStrictEqual(new MyDate(5), new Date(5), true)));
out("skip-u8", attempt(() => util.isDeepStrictEqual(new MyU8([1]), new Uint8Array([1]), true)));
out("skip-string", attempt(() => util.isDeepStrictEqual(new MyString("a"), new String("a"), true)));
out("skip-error", attempt(() => util.isDeepStrictEqual(new MyError("a"), new Error("a"), true)));
out("skip-weakmap", attempt(() => util.isDeepStrictEqual(new WeakMap(), new WeakMap(), true)));
