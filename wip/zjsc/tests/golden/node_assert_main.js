// node:assert e node:assert/strict contra o bun 1.4.2: forma do módulo, cada asserção passando e falhando, a mensagem
// do AssertionError (com o diff de Myers), partialDeepStrictEqual, throws/rejects e o CallTracker.
const assert = require("node:assert");
const strict = require("node:assert/strict");

function show(label, fn) {
  try {
    const value = fn();
    console.log(label, "ok", value === undefined ? "" : value);
  } catch (e) {
    console.log(label, "throws", e.name, e.code, JSON.stringify(e.message));
    if (e instanceof assert.AssertionError) {
      console.log("  ", e.generatedMessage, e.operator, typeof e.actual, typeof e.expected);
    }
  }
}

console.log(typeof assert, assert.name, assert.length, Object.keys(assert).join());
console.log(typeof strict, strict.name, strict === assert.strict, strict.strict === strict, Object.keys(strict).join());
console.log(require("assert") === assert, require("assert/strict") === strict);
console.log(assert.AssertionError.name, typeof assert.Assert, assert.Assert.name);
for (const name of ["ok", "equal", "deepStrictEqual", "partialDeepStrictEqual", "throws", "rejects", "match", "ifError", "fail"]) {
  console.log(name, assert[name].name, assert[name].length, strict[name] === assert[name]);
}
console.log(strict.equal === assert.strictEqual, strict.deepEqual === assert.deepStrictEqual);

show("ok", () => assert.ok(1));
show("ok false", () => assert.ok(0));
show("ok none", () => assert.ok());
show("ok msg", () => assert(false, "custom message"));
show("ok error", () => assert(false, new RangeError("boom")));
show("equal", () => assert.equal(1, "1"));
show("equal fail", () => assert.equal(1, 2));
show("notEqual", () => assert.notEqual(1, 1));
show("strictEqual", () => assert.strictEqual(1, "1"));
show("strictEqual str", () => assert.strictEqual("hello world", "hello there"));
show("strictEqual lines", () => assert.strictEqual("a\nb\nc", "a\nB\nc"));
show("strictEqual NaN", () => assert.strictEqual(NaN, NaN));
show("notStrictEqual", () => assert.notStrictEqual(2, 2));
show("deepEqual", () => assert.deepEqual({ a: 1 }, { a: "1" }));
show("deepEqual fail", () => assert.deepEqual({ a: 1 }, { a: 2 }));
show("deepStrictEqual", () => assert.deepStrictEqual({ a: 1, b: [1, 2] }, { a: 1, b: [1, 2] }));
show("deepStrictEqual fail", () => assert.deepStrictEqual({ a: 1, b: [1, 2, 3] }, { a: 1, b: [1, 5, 3] }));
show("deepStrictEqual long", () =>
  assert.deepStrictEqual(
    Array.from({ length: 30 }, (_, i) => i),
    Array.from({ length: 30 }, (_, i) => (i === 20 ? -1 : i)),
  ),
);
show("deepStrictEqual proto", () => assert.deepStrictEqual(Object.create(null), {}));
show("notDeepStrictEqual", () => assert.notDeepStrictEqual([1], [1]));
show("notDeepEqual", () => assert.notDeepEqual({ a: 1 }, { a: 1 }));
show("fail", () => assert.fail());
show("fail msg", () => assert.fail("nope"));
show("fail error", () => assert.fail(new TypeError("t")));
show("ifError null", () => assert.ifError(null));
show("ifError", () => assert.ifError(new Error("inner")));
show("ifError value", () => assert.ifError("x"));
show("match", () => assert.match("abc", /b/));
show("match fail", () => assert.match("abc", /z/));
show("match bad", () => assert.match(1, /z/));
show("doesNotMatch", () => assert.doesNotMatch("abc", /b/));
show("missing args", () => assert.strictEqual(1));
show("missing args 2", () => assert.deepStrictEqual());

show("partial ok", () => assert.partialDeepStrictEqual({ a: 1, b: { c: 2, d: 3 } }, { b: { c: 2 } }));
show("partial fail", () => assert.partialDeepStrictEqual({ a: 1 }, { a: 2 }));
show("partial array", () => assert.partialDeepStrictEqual([1, 2, 3, 4], [2, 4]));
show("partial array order", () => assert.partialDeepStrictEqual([1, 2, 3], [3, 1]));
show("partial set", () => assert.partialDeepStrictEqual(new Set([1, { a: 1, b: 2 }]), new Set([{ a: 1 }])));
show("partial map", () => assert.partialDeepStrictEqual(new Map([["a", { x: 1, y: 2 }], ["b", 2]]), new Map([["a", { x: 1 }]])));
show("partial map fail", () => assert.partialDeepStrictEqual(new Map([["a", 1]]), new Map([["b", 1]])));
show("partial typed", () => assert.partialDeepStrictEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 3])));
show("partial float NaN", () => assert.partialDeepStrictEqual(new Float64Array([NaN, 0]), new Float64Array([NaN, -0])));
show("partial error", () => assert.partialDeepStrictEqual(new Error("m"), new Error("")));
show("partial date", () => assert.partialDeepStrictEqual(new Date(5), new Date(5)));
show("partial zero", () => assert.partialDeepStrictEqual({ a: 0 }, { a: -0 }));
show("partial boxed", () => assert.partialDeepStrictEqual(new Number(1), new Number(1)));
const cyc1 = { a: 1 };
cyc1.self = cyc1;
const cyc2 = { };
cyc2.self = cyc2;
show("partial cycle", () => assert.partialDeepStrictEqual(cyc1, cyc2));

show("throws", () => assert.throws(() => { throw new TypeError("bad"); }, TypeError));
show("throws regexp", () => assert.throws(() => { throw new Error("bad thing"); }, /thing/));
show("throws object", () => assert.throws(() => { throw new Error("bad"); }, { message: "bad", name: "Error" }));
show("throws object fail", () => assert.throws(() => { throw new Error("bad"); }, { message: "good" }));
show("throws none", () => assert.throws(() => {}));
show("throws none msg", () => assert.throws(() => {}, "should throw"));
show("throws wrong type", () => assert.throws(() => { throw new RangeError("r"); }, TypeError));
show("throws validate", () => assert.throws(() => { throw new Error("v"); }, e => e.message === "v"));
show("throws ambiguous", () => assert.throws(() => { throw new Error("same"); }, "same"));
show("throws bad arg", () => assert.throws(() => {}, 5));
show("throws empty", () => assert.throws(() => { throw new Error("e"); }, {}));
show("doesNotThrow", () => assert.doesNotThrow(() => {}));
show("doesNotThrow fail", () => assert.doesNotThrow(() => { throw new Error("x"); }));
show("doesNotThrow fail msg", () => assert.doesNotThrow(() => { throw new Error("x"); }, "msg"));

const e = new assert.AssertionError({ actual: 1, expected: 2, operator: "strictEqual" });
console.log(e.name, e.code, e.actual, e.expected, e.operator, e.generatedMessage, JSON.stringify(e.message));
console.log(Object.keys(e).join(), String(e).split("\n")[0]);
const e2 = new assert.AssertionError({ message: "plain" });
console.log(e2.message, e2.generatedMessage, e2.operator);
show("AssertionError bad", () => new assert.AssertionError(5));

const custom = new assert.Assert({ strict: false });
show("Assert loose", () => custom.equal(1, "1"));
show("Assert no new", () => assert.Assert());
show("Assert diff", () => new assert.Assert({ diff: "x" }));

const tracker = new assert.CallTracker();
const tracked = tracker.calls(() => 1, 2);
tracked();
console.log(tracker.report().map(r => [r.message, r.actual, r.expected].join("|")).join());
show("verify", () => tracker.verify());
tracked();
show("verify ok", () => tracker.verify());
show("calls bad", () => tracker.calls(() => {}, 0));

(async () => {
  for (const [label, run] of [
    ["rejects", () => assert.rejects(Promise.reject(new Error("r")), Error)],
    ["rejects fn", () => assert.rejects(async () => { throw new TypeError("t"); }, TypeError)],
    ["rejects none", () => assert.rejects(Promise.resolve(1))],
    ["rejects bad", () => assert.rejects(5)],
    ["rejects bad return", () => assert.rejects(() => 5)],
    ["doesNotReject", () => assert.doesNotReject(Promise.resolve(1))],
    ["doesNotReject fail", () => assert.doesNotReject(Promise.reject(new Error("no")))],
  ]) {
    try {
      await run();
      console.log(label, "ok");
    } catch (err) {
      console.log(label, "rejects", err.name, err.code, JSON.stringify(err.message));
    }
  }
})();
