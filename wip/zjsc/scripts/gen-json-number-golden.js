// Gera tests/golden/json_number_bun.tsv: JSON.parse/stringify e conversões numéricas, avaliados no bun.
// Colunas: fonte (uma linha), depois o resultado do harness tests/golden/json_number_bun_harness.js.
// Uso: timeout 300 bun scripts/gen-json-number-golden.js > tests/golden/json_number_bun.tsv
const fs = require("fs");
const path = require("path");
const { sampleByHash } = require("./golden-prelude.js");

const harness = (0, eval)(fs.readFileSync(path.join(__dirname, "../tests/golden/json_number_bun_harness.js"), "utf8").trimEnd());

const programs = [];
const add = (...sources) => programs.push(...sources);

/** Literal de string JS numa linha só, todo ASCII salvo o que for imprimível. */
function q(text) {
  return JSON.stringify(text).replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029").replace(/\u0085/g, "\\u0085");
}
function lit(x) {
  if (Object.is(x, -0)) return "-0";
  return String(x);
}

// PRNG determinístico.
let seed = 0x9e3779b9;
function rnd() {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const f64 = new Float64Array(1);
const u64 = new BigUint64Array(f64.buffer);
function randomDouble() {
  for (;;) {
    u64[0] = (BigInt(Math.floor(rnd() * 4294967296)) << 32n) | BigInt(Math.floor(rnd() * 4294967296));
    if (Number.isFinite(f64[0])) return f64[0];
  }
}
function randomSmall() {
  const digits = 1 + Math.floor(rnd() * 8);
  let x = Math.floor(rnd() * 10 ** digits);
  const scale = Math.floor(rnd() * 12) - 6;
  return x * 10 ** scale;
}

// O PRNG só GERA candidatos: `fillByHash` pede ao gerador o triplo do que falta para `size` e a escolha entre os candidatos é
// feita por `sampleByHash`, pelo texto de cada um, nunca pela posição nem pelo fluxo do PRNG.
function fillByHash(list, size, generate, keyOf = lit) {
  const need = size - list.length;
  if (need <= 0) return;
  const seenKeys = new Set(list.map(keyOf));
  const candidates = [];
  for (let n = 0; n < need * 3; n++) {
    const candidate = generate();
    const key = keyOf(candidate);
    if (!seenKeys.has(key)) { seenKeys.add(key); candidates.push(candidate); }
  }
  list.push(...sampleByHash(candidates, need, keyOf));
}

// ---------------------------------------------------------------- JSON.parse: erros
const badTexts = [
  "", " ", "\n", "{", "}", "[", "]", ",", ":", "{,}", "[,]", "[1,]", "[,1]", "{\"a\":1,}", "{\"a\"}", "{\"a\":}", "{a:1}", "{'a':1}",
  "{\"a\" 1}", "{\"a\":1 \"b\":2}", "[1 2]", "[1,,2]", "undefined", "NaN", "Infinity", "-Infinity", "-", "+1", "01", "00", "-01", "1.", ".5",
  "1e", "1e+", "1E-", "0x10", "0b1", "1_000", "1n", "tru", "True", "nul", "nulll", "falsy", "'a'", "\"a", "\"a\nb\"", "\"a\tb\"",
  "\"\\x41\"", "\"\\u12\"", "\"\\u12G4\"", "\"\\", "\"\\a\"", "\"\\'\"", "\"\\0\"", "[1] x", "[1]]", "{} {}", "1 2", "// c\n1", "/* c */ 1",
  "\u00a01", "1\u00a0", "\ufeff1", "\u20281", "\u2029[]", "\u000b1", "\u000c1", "[1,2", "[1,2,", "{\"a\":1", "{\"a\":1,", "{\"a\":[}", "[{]",
  "[}", "{]", "\"\u0000\"", "\"\u001f\"", "\u0000", "@", "#", "`a`", "function", "(1)", "1;", "[1];", "{\"a\":1};", "\"\\ud800\"x", "\"abc\\",
  "\"\\u00\"", "\"\\u\"", "-a", "--1", "1-", "1..2", "1.2.3", "1e5e5", "[1e]", "{\"a\":1e}", "[.]", "[-]", "[+]", "{\"\":}", "{\"a\":\"b\"",
  "{\"a\":\"b", "[\"a\",", "[\"a\"", "[tru]", "[nul]", "[fals]", "[truee]", "[Null]", "{\"a\":nan}", "{\"a\":undefined}", "[undefined]", "[NaN]",
  "[Infinity]", "[-Infinity]", "[0x1]", "[01]", "[1.]", "[.1]", "[1,]", "{,\"a\":1}", "{\"a\":1,,\"b\":2}", "[[[[", "]]]]", "{{", "[\"\\u{41}\"]",
  "\"\\U0041\"", "\"\\/\"x", "\u00e9", "\"\u00e9", "[\u00e9]", "{\"\u00e9\":}", "\ud83d\ude00", "\"\ud83d", "[\"\ud800\"", "null null", "true false", "\"a\" \"b\"",
  "[1]\u0000", "\u0000[1]", "[1,2,3,4,5,6,7,8,9,10,", "{\"k\":{\"k\":{\"k\":", "[[1,2],[3,4],", "[\"a\" \"b\"]", "{\"a\":1 \"b\"}", "{\"a\":1:2}", "[1:2]",
  "{\"a\":[1,2}", "{\"a\":{\"b\":1]}", "[{\"a\":1]", "{1:2}", "{null:1}", "{true:1}", "{\"a\":1,\"b\"}", "{\"a\":1,\"b\":}", "{\"a\":1,\"b\":,}",
  "-0x1", "0.0.0", "1e1.5", "1E+", "\"\\u000\"", "\"\\u0g00\"", "\"\\uD83D\\u\"", "\"\\\r\"", "\"\\\n\"", "\"\\\u2028\"", "\u00ff", "\u0100", "\uffff", "\ufffe",
  " \t\r\n ", "\t\t[", "\r\n}", "  {  \"a\" :  ", "[ 1 , 2 , ", "[ true , fals ]", "[ \"x\" , 'y' ]", "{ \"a\" : 1 , \"b\" : 2 , }", "[ 1 , 2 , ]",
];
for (const t of badTexts) add(`JSON.parse(${q(t)})`);

// Prefixos e trocas de caractere de documentos válidos: posição e token em cada ponto.
const docs = ['{"a":[1,2.5e3,{"b":null}],"c":"x\\n\\u0041","d":true,"e":false}', '[1,"two",[3,[4]],{"k":-0.5}]', '"str\\u00e9\\ud83d\\ude00"'];
for (const doc of docs) {
  for (let i = 0; i < doc.length; i++) {
    add(`JSON.parse(${q(doc.slice(0, i))})`);
    add(`JSON.parse(${q(doc.slice(0, i) + "@" + doc.slice(i + 1))})`);
  }
  add(`JSON.parse(${q(doc + "x")})`, `JSON.parse(${q(doc + " 1")})`, `JSON.parse(${q(doc)}).constructor.name`);
}

// ---------------------------------------------------------------- JSON.parse: sucessos
const goodTexts = [
  "null", "true", "false", "0", "-0", "1", "-1", "1.5", "1e3", "1E3", "1e+3", "1e-3", "-1.5e-10", "0.1", "123456789012345678901234567890", "1e400", "-1e400",
  "1e-400", "5e-324", "2.2250738585072014e-308", "1.7976931348623157e308", "1.7976931348623159e308", "9007199254740993", "0.30000000000000004",
  "[]", "{}", " [ ] ", "[1,2,3]", "[[],[[]],{}]", "{\"a\":1}", "{\"a\":1,\"a\":2}", "{\"b\":1,\"a\":2,\"1\":3,\"0\":4}", "{\"\":1}", "{\"__proto__\":1}",
  "{\"__proto__\":{\"x\":1}}", "{\"a\":{\"__proto__\":null}}", "[{\"__proto__\":[]}]", "\"\"", "\"abc\"", "\"a\\\"b\"", "\"\\\\\"", "\"\\/\"", "\"\\b\\f\\n\\r\\t\"",
  "\"\\u0041\\u00e9\\u20ac\"", "\"\\ud83d\\ude00\"", "\"\\ud83d\"", "\"\\ude00\"", "\"\\ud83dx\"", "\"x\\ude00\\ud83d\"", "\"\\uD83D\\uDE00\\uD83D\"", "\"\u2028\u2029\"",
  "\"\u00e9\ud83d\ude00\"", "\"\\u0000\"", "\"\u007f\"", "\"\\u001F\"", "\t\n\r 1 \t\n\r", "[\n1\n,\n2\n]", "{\"a\":[{\"b\":[{\"c\":null}]}]}", "\"\ud800\"", "\"\udc00\"",
  "[1e1,1E1,0e0,0.0,-0.0,0e-0]", "[0.1e1,0.12e-1]", "100000000000000000000", "1000000000000000000000", "0.000001", "0.0000001", "123e-20",
  "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "-9223372036854775808", "18446744073709551616",
  "[1.0000000000000002,1.00000000000000011]", "0.1e-5", "1e0000000000000000000000001", "1e-0000000000000000000000001",
];
for (const t of goodTexts) add(`JSON.stringify(JSON.parse(${q(t)}))`, `Object.is(JSON.parse(${q(t)}), -0)`);
for (const t of ["{\"__proto__\":1}", "{\"__proto__\":{\"x\":1}}", "{\"a\":{\"__proto__\":null}}", "{\"a\":1,\"__proto__\":2,\"b\":3}"]) {
  add(
    `var o = JSON.parse(${q(t)}); Object.keys(o).join() + "|" + (Object.getPrototypeOf(o) === Object.prototype) + "|" + Object.getOwnPropertyNames(o).join()`,
    `var o = JSON.parse(${q(t)}); Object.prototype.hasOwnProperty.call(o, "__proto__")`,
    `var o = JSON.parse(${q(t)}); typeof Object.getOwnPropertyDescriptor(o, "__proto__")`,
  );
}
add(
  `JSON.parse("1e400") === Infinity`, `JSON.parse("-1e400") === -Infinity`, `JSON.parse("1e-400") === 0`, `Object.is(JSON.parse("-1e-400"), -0)`,
  `JSON.parse("123456789012345678901234567890") === 1.2345678901234568e29`, `JSON.parse("[" + "9".repeat(400) + "]")[0]`,
  `JSON.parse("0." + "0".repeat(400) + "1")`, `JSON.parse("0." + "9".repeat(400))`, `JSON.parse("1" + "0".repeat(308))`, `JSON.parse("1" + "0".repeat(309))`,
  `JSON.parse("1." + "0".repeat(1000) + "1e5")`, `JSON.parse("0." + "0".repeat(1000) + "1e1000")`, `JSON.parse("1e" + "0".repeat(1000) + "5")`,
  `JSON.parse(" ".repeat(100000) + "1")`, `JSON.parse("\\"" + "a".repeat(100000) + "\\"").length`, `JSON.parse("[" + "1,".repeat(100000) + "1]").length`,
  `Object.keys(JSON.parse("{" + Array.from({length: 1000}, (_, i) => '"k' + i + '":' + i).join(",") + "}")).length`,
  `JSON.parse("1", undefined)`, `JSON.parse(1)`, `JSON.parse(null)`, `JSON.parse(true)`, `JSON.parse([1])`, `JSON.parse([])`, `JSON.parse({})`, `JSON.parse()`,
  `JSON.parse(undefined)`, `JSON.parse(Symbol())`, `JSON.parse(1n)`, `JSON.parse({toString() { return "[7]" }})[0]`, `JSON.parse(new String("8"))`,
  `JSON.parse("1", 5)`, `JSON.parse("[1]", null)`, `JSON.parse("[1]", {})`, `JSON.parse.length`, `JSON.parse.name`, `JSON[Symbol.toStringTag]`,
);

// Profundidade de aninhamento.
for (const n of [100, 1000, 5000, 9999, 10000, 10001, 20000, 50000, 100000, 1000000]) {
  add(`typeof JSON.parse("[".repeat(${n}) + "]".repeat(${n}))`);
  add(`typeof JSON.parse('{"a":'.repeat(${n}) + "1" + "}".repeat(${n}))`);
  add(`typeof JSON.parse("[".repeat(${n}))`);
  add(`typeof JSON.stringify(JSON.parse("[".repeat(${n}) + "]".repeat(${n})))`);
  add(`(function () { var a = []; for (var i = 0; i < ${n}; i++) a = [a]; return JSON.stringify(a).length })()`);
}

// ---------------------------------------------------------------- reviver
add(
  `var log = []; JSON.parse('{"a":[1,{"b":2}],"c":3}', function (k, v) { log.push(JSON.stringify(k) + ":" + JSON.stringify(v)); return v }); log.join(" ")`,
  `var log = []; JSON.parse('{"a":[1,{"b":2}],"c":3}', function (k, v) { log.push(typeof this + ":" + Array.isArray(this)); return v }); log.join(" ")`,
  `JSON.stringify(JSON.parse('{"a":1,"b":2,"c":3}', function (k, v) { return k === "b" ? undefined : v }))`,
  `JSON.stringify(JSON.parse('[1,2,3]', function (k, v) { return k === "1" ? undefined : v }))`,
  `JSON.stringify(JSON.parse('[1,2,3]', function (k, v) { return typeof v === "number" ? v * 2 : v }))`,
  `JSON.stringify(JSON.parse('{"a":{"b":{"c":1}}}', function (k, v) { return k === "" ? "root" : v }))`,
  `JSON.parse('7', function (k, v) { return [k, v] }).join()`,
  `var log = []; JSON.parse('{"a":1,"b":2}', function (k, v) { if (k === "a") this.b = 9; log.push(k + "=" + v); return v }); log.join()`,
  `var log = []; JSON.parse('{"a":1,"b":2}', function (k, v) { if (k === "a") delete this.b; log.push(k + "=" + v); return v }); log.join()`,
  `var log = []; JSON.parse('[1,2,3]', function (k, v) { if (k === "0") this.length = 1; log.push(k); return v }); log.join()`,
  `JSON.parse('{"a":1}', function (k, v) { throw new RangeError("boom") })`,
  `var log = []; JSON.parse('{"a":1,"b":[10,20],"c":"s","d":null,"e":true,"f":1.50,"g":1e2}', function (k, v, ctx) { log.push(k + ":" + JSON.stringify(ctx)); return v }); log.join(" ")`,
  `var log = []; JSON.parse('[1.0, 2e0, -0, "x", true, null, {}, []]', function (k, v, ctx) { log.push(k + ":" + JSON.stringify(ctx)); return v }); log.join(" ")`,
  `var log = []; JSON.parse('{"a":[1,2],"b":{"c":3}}', function (k, v, ctx) { log.push(k + ":" + Object.keys(ctx).join()); return v }); log.join(" ")`,
  `var log = []; JSON.parse('{"a":1}', function (k, v, ctx) { if (k === "a") this.a = 2; log.push(k + ":" + v + ":" + JSON.stringify(ctx)); return v }); log.join(" ")`,
  `var log = []; JSON.parse('123456789012345678901234567890', function (k, v, ctx) { log.push(JSON.stringify(ctx) + typeof v); return v }); log.join(" ")`,
  `var log = []; JSON.parse('"\\\\u0041bc"', function (k, v, ctx) { log.push(JSON.stringify(ctx)); return v }); log.join(" ")`,
  `var log = []; JSON.parse('{"__proto__":1}', function (k, v) { log.push(k); return v }); log.join()`,
  `JSON.stringify(JSON.parse('{"a":1}', function (k, v) { return k === "" ? v : {x: v} }))`,
  `JSON.stringify(JSON.parse('[[1],[2]]', function (k, v) { return Array.isArray(v) ? v.length : v }))`,
  `JSON.parse('1', function () { return 1n })`,
  `typeof JSON.parse('1', function () { return Symbol() })`,
  `JSON.stringify(JSON.parse('{"a":1,"b":2}', function (k, v) { if (k === "a") Object.defineProperty(this, "b", {configurable: false, value: 5}); return k === "b" ? undefined : v }))`,
  `JSON.stringify(JSON.parse('{"a":1}', function (k, v) { return v }, 123))`,
  `JSON.stringify(JSON.parse('{"a":1}', {call() {}}))`,
  `JSON.stringify(JSON.parse('[1,2]', new Proxy(function (k, v) { return v }, {})))`,
  `var p = new Proxy([], {}); JSON.stringify(JSON.parse('[1,2]', function (k, v) { return k === "" ? p : v }))`,
);

// ---------------------------------------------------------------- JSON.rawJSON / isRawJSON
for (const t of ["1", "-0", "1.5e10", "\"x\"", "null", "true", "false", "123456789012345678901234567890", "\"\\u0041\"", "", " 1", "1 ", "\n1", "1\t", "[]", "{}", "[1]", "{\"a\":1}", "abc", "1n", "NaN", "Infinity", "01", "\"", "\"a\nb\""]) {
  add(`JSON.stringify(JSON.rawJSON(${q(t)}))`, `JSON.stringify({a: JSON.rawJSON(${q(t)})})`, `JSON.isRawJSON(JSON.rawJSON(${q(t)}))`);
}
add(
  `Object.isFrozen(JSON.rawJSON("1"))`, `Object.getPrototypeOf(JSON.rawJSON("1"))`, `Object.keys(JSON.rawJSON("1")).join()`, `JSON.rawJSON("1").rawJSON`,
  `Object.getOwnPropertyNames(JSON.rawJSON("1")).join()`, `Object.prototype.toString.call(JSON.rawJSON("1"))`, `JSON.rawJSON.length`, `JSON.isRawJSON.length`,
  `JSON.rawJSON.name`, `JSON.isRawJSON.name`, `JSON.rawJSON()`, `JSON.rawJSON(undefined)`, `JSON.rawJSON(1)`, `JSON.rawJSON(null)`, `JSON.rawJSON(Symbol())`,
  `JSON.rawJSON(1n)`, `JSON.rawJSON({})`, `JSON.isRawJSON()`, `JSON.isRawJSON({})`, `JSON.isRawJSON({rawJSON: "1"})`, `JSON.isRawJSON(1)`, `JSON.isRawJSON(null)`,
  `JSON.stringify([JSON.rawJSON("1e1000"), JSON.rawJSON("\\"a\\"")])`, `JSON.stringify(JSON.rawJSON("1"), null, 2)`, `JSON.stringify({a: [JSON.rawJSON("1")]}, null, 2)`,
  `JSON.stringify({a: 1}, function (k, v) { return k === "a" ? JSON.rawJSON("9007199254740993") : v })`, `JSON.stringify({a: 1}, ["a"]) + JSON.stringify(JSON.rawJSON("2"), ["a"])`,
  `JSON.stringify({toJSON() { return JSON.rawJSON("5") }})`, `JSON.stringify(JSON.rawJSON("1"), function (k, v) { return typeof v })`,
  `var o = JSON.rawJSON("1"); o.x = 1; o.x`, `"use strict"; var o = JSON.rawJSON("1"); o.x = 1`, `"use strict"; var o = JSON.rawJSON("1"); delete o.rawJSON`,
  `String(JSON.rawJSON("1"))`, `JSON.rawJSON("1") + ""`, `Object.getOwnPropertyDescriptor(JSON.rawJSON("1"), "rawJSON").writable`,
);

// ---------------------------------------------------------------- JSON.stringify
const values = [
  "undefined", "null", "true", "false", "0", "-0", "1", "-1.5", "1e21", "1e-7", "NaN", "Infinity", "-Infinity", "5e-324", "1.7976931348623157e308", "123456789012345680000",
  "''", "'a'", "'a\"b\\\\c'", "'\\n\\r\\t\\b\\f\\v\\0'", "'\\u2028\\u2029'", "'\\u007f\\u0080\\u00ff'", "'\\ud800'", "'\\udc00'", "'\\ud83d\\ude00'", "'\\ud83d'", "'x\\ude00y'", "'\\udc00\\ud800'",
  "'\\ud83d\\ud83d\\ude00'", "'\\ude00\\ud83d\\ude00'", "'\\ud83d\\ude00\\ud83d'", "'\\u0000\\u001f'", "'</script>'", "'\\u00e9\\u20ac'",
  "Symbol('s')", "function () {}", "() => 1", "class A {}", "10n", "[]", "[1,2,3]", "[undefined, function () {}, Symbol()]", "[NaN, Infinity, -0]", "[,1]", "[1,,2]", "new Array(3)",
  "{}", "{a: 1}", "{a: undefined}", "{a: function () {}}", "{a: Symbol()}", "{[Symbol('k')]: 1}", "{b: 1, a: 2, 1: 3, 0: 4}", "{a: {b: {c: [1, {d: 2}]}}}", "{'': 1}", "{'\\n': 1}", "{'\\ud800': 1}",
  "new Number(3)", "new String('s')", "new Boolean(false)", "Object(1n)", "Object(Symbol())", "new Number(NaN)", "new Number(-0)", "new String('\\ud800')",
  "new Date(0)", "new Date(NaN)", "new Date(8.64e15)", "new Date(-62198755200000)", "new Date(253402300800000)", "new Date(1e12)",
  "new Map([[1, 2]])", "new Set([1])", "new WeakMap", "new WeakSet", "new Error('x')", "new RangeError('x')", "/re/g", "new Uint8Array([1, 2])", "new Float32Array([1.5, NaN])",
  "new ArrayBuffer(4)", "new DataView(new ArrayBuffer(1))", "Promise.resolve(1)", "Math", "JSON", "globalThis.Reflect", "Object.create(null)", "Object.create({inherited: 1})", "Object.create({}, {own: {value: 1, enumerable: true}, hid: {value: 2}})",
  "{toJSON() { return 'tj' }}", "{toJSON: 1, a: 2}", "{toJSON() { return undefined }}", "{toJSON() { return {x: this.y}; }, y: 7}", "{toJSON(k) { return 'key:' + k }}", "[{toJSON(k) { return 'key:' + k }}]", "{a: {toJSON(k) { return 'key:' + k }}}",
  "{get a() { return 5 }}", "{get a() { throw new RangeError('getter') }}", "new Proxy({a: 1}, {})", "new Proxy([1, 2], {})", "new Proxy(function () {}, {})", "new Proxy({}, {ownKeys() { return ['x'] }, getOwnPropertyDescriptor() { return {value: 1, enumerable: true, configurable: true} }, get() { return 9 }})",
  "(function () { return arguments })(1, 2)", "Object.assign([1, 2], {x: 1})", "Object.assign(() => {}, {x: 1})", "Object.assign(function () {}, {toJSON() { return 4 }})", "[new Number(1), new String('a'), new Boolean(true), Object(2n)]".replace(", Object(2n)", ""),
  "Object.defineProperty({}, 'a', {value: 1, enumerable: false})", "Object.defineProperty({a: 1}, 'b', {get() { return 2 }, enumerable: true})", "Object.freeze({a: 1})", "Object.seal([1])",
  "new (class A { x = 1; #p = 2; })", "new (class B extends Array {})", "Object.setPrototypeOf({a: 1}, null)", "new (class E { toJSON() { return 'cls' } })", "Symbol.iterator", "globalThis.nonexistent",
  "[[[[[[[[[[1]]]]]]]]]]", "{a: [{b: [{c: []}]}]}", "[{}, [], '', 0, null]", "{a: [], b: {}}", "[[], {}]", "[[1, [2, [3]]], {a: {b: 1}}]",
];
for (const v of values) add(`JSON.stringify(${v})`);

const spacers = ["2", "4", "10", "11", "100", "0", "-1", "1.9", "0.9", "NaN", "Infinity", "'  '", "'\\t'", "'abc'", "'abcdefghijkl'", "'abcdefghij'", "''", "'\\ud83d\\ude00'", "'\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d'", "new Number(3)", "new String('--')", "new Number(20)", "true", "null", "undefined", "{}", "[]", "Symbol()", "1e100", "-Infinity", "'\\n'", "new Boolean(true)", "3n".replace("3n", "3")];
const spaceTargets = ["{a: 1, b: [1, 2, {c: 3}], d: {}, e: []}", "[1, [2, [3]], {}]", "[]", "{}", "1", "'s'", "{a: {b: {c: {d: 1}}}}", "[{}, [], [[]]]"];
for (const s of spacers) for (const t of spaceTargets) add(`JSON.stringify(${t}, null, ${s})`);
add(`JSON.stringify({a: 1}, null, 5n)`, `JSON.stringify({a: 1}, null, {valueOf() { return 2 }})`, `JSON.stringify({a: 1}, null, {toString() { return "~" }})`, `JSON.stringify([1], null, new Proxy(new Number(2), {}))`);

// replacer
const replTarget = "{a: 1, b: [1, 2, {a: 3, c: 4}], c: {a: 5, d: 6}, 1: 7}";
for (const r of [
  "function (k, v) { return v }", "function (k, v) { return typeof v === 'number' ? v + 1 : v }", "function (k, v) { return k === 'a' ? undefined : v }", "function (k, v) { return k === '' ? [v] : v }",
  "function (k, v) { return Array.isArray(v) ? v.length : v }", "function (k, v) { return k === 'b' ? 'B' : v }", "function (k, v) { return this === undefined ? 'u' : v }", "function (k, v) { return typeof this + (k === '' ? '!' : '') + (typeof v === 'object' ? '' : v) }",
  "function () { return undefined }", "function () { return null }", "function () { return 5 }", "function () { return Symbol() }", "function () { return function () {} }", "function () { throw new TypeError('rep') }",
  "function (k, v) { return v === 6 ? 10n : v }", "function (k, v) { if (k === 'a') this.x = 1; return v }", "function (k, v) { return k === 'c' ? {toJSON() { return 'cj' }} : v }",
  "['a']", "['a', 'c']", "['c', 'a']", "['a', 'a']", "['a', 1]", "[1]", "['1', 1, 'a']", "[new String('a'), new Number(1)]", "[]", "['zzz']", "['a', {}]", "['a', null, undefined, Symbol()]", "[1.5]", "['b', 'a', 'c', 'd']",
  "new Proxy(['a'], {})", "{}", "null", "undefined", "1", "'a'", "true", "[,'a']",
]) add(`JSON.stringify(${replTarget}, ${r})`, `JSON.stringify([1, {a: 2, b: 3}], ${r}, 1)`);
add(
  `JSON.stringify({a: 1}, function (k, v) { return k === "" ? 1 : v })`, `var log = []; JSON.stringify({a: [1, {b: 2}], c: 3}, function (k, v) { log.push(JSON.stringify(k)); return v }); log.join()`,
  `var log = []; JSON.stringify([{toJSON(k) { log.push("tj" + k); return 1 }}], function (k, v) { log.push("r" + k); return v }); log.join()`,
  `JSON.stringify({a: 1}, function (k, v) { return k === "" ? new Number(7) : v })`, `JSON.stringify({a: 1}, function (k, v) { return k === "" ? new String("s") : v })`,
  `JSON.stringify({a: 1}, function (k, v) { return k === "" ? new Boolean(false) : v })`, `JSON.stringify({a: 1}, function (k, v) { return k === "" ? Object(5n) : v })`,
  `JSON.stringify({a: 1}, function (k, v) { return k === "" ? undefined : v })`, `JSON.stringify(undefined, function () { return 1 })`,
  `JSON.stringify({a: 1}, 1, 2)`, `JSON.stringify({a: [1]}, "str", 2)`, `JSON.stringify.length`, `JSON.stringify.name`,
);

// toJSON, BigInt, Date
add(
  `JSON.stringify(1n)`, `JSON.stringify([1n])`, `JSON.stringify({a: 1n})`, `JSON.stringify({a: {b: [1, 2n]}})`, `JSON.stringify(Object(1n))`, `JSON.stringify({a: Object(1n)})`,
  `JSON.stringify(1n, function (k, v) { return String(v) })`, `JSON.stringify({a: 1n}, function (k, v) { return typeof v === "bigint" ? "big" + v : v })`, `JSON.stringify({a: 1n}, ["b"])`, `JSON.stringify({a: 1n}, ["a"])`,
  `BigInt.prototype.toJSON = function () { return "tj" + this }; var r = JSON.stringify([1n, {a: 2n}, Object(3n)]); delete BigInt.prototype.toJSON; r`,
  `BigInt.prototype.toJSON = function () { return typeof this }; var r = JSON.stringify(1n); delete BigInt.prototype.toJSON; r`,
  `JSON.stringify({toJSON() { return 1n }})`, `JSON.stringify(BigInt(2) ** 64n)`, `JSON.stringify({a: -(2n ** 100n)})`,
  `JSON.stringify(new Date(0))`, `JSON.stringify({d: new Date(1e12)})`, `JSON.stringify(new Date(NaN))`, `JSON.stringify({d: new Date(NaN)})`, `JSON.stringify(new Date(-1))`, `JSON.stringify(new Date(-62198755200000))`,
  `JSON.stringify(new Date(-62198755200001))`, `JSON.stringify(new Date(253402300799999))`, `JSON.stringify(new Date(253402300800000))`, `JSON.stringify(new Date(8.64e15))`, `JSON.stringify(new Date(-8.64e15))`,
  `var d = new Date(0); d.toJSON = function (k) { return "k=" + k }; JSON.stringify({a: d})`, `Date.prototype.toJSON.call({toISOString() { return "iso" }})`, `Date.prototype.toJSON.call({valueOf() { return NaN }, toISOString() { return "x" }})`,
  `Date.prototype.toJSON.call({valueOf() { return Infinity }})`, `Date.prototype.toJSON.call({toISOString: 1})`, `Date.prototype.toJSON.call(1)`, `Date.prototype.toJSON.call(undefined)`, `Date.prototype.toJSON.call({valueOf() { return 1 }})`,
  `Date.prototype.toJSON.call({toISOString() { return 5 }, valueOf() { return 0 }})`, `Date.prototype.toJSON.length`,
  `JSON.stringify({toJSON: function () { return this }})`, `JSON.stringify({a: 1, toJSON: function () { return {b: 2, toJSON: function () { return 3 }} }})`,
  `JSON.stringify({toJSON() { throw new SyntaxError("tj") }})`, `JSON.stringify({toJSON: null})`, `JSON.stringify({toJSON: {}})`, `JSON.stringify({toJSON: "x"})`, `JSON.stringify(Object.create({toJSON() { return "inh" }}))`,
  `Number.prototype.toJSON = function () { return "n" }; var r = JSON.stringify([1, new Number(2)]); delete Number.prototype.toJSON; r`,
  `String.prototype.toJSON = function () { return "s" }; var r = JSON.stringify(["a", new String("b")]); delete String.prototype.toJSON; r`,
  `Boolean.prototype.toJSON = function () { return typeof this }; var r = JSON.stringify([true]); delete Boolean.prototype.toJSON; r`,
  `Symbol.prototype.toJSON = function () { return "sym" }; var r = JSON.stringify([Symbol()]); delete Symbol.prototype.toJSON; r`,
  `Object.prototype.toJSON = function () { return "obj" }; var r = JSON.stringify({a: 1}); delete Object.prototype.toJSON; r`,
  `Array.prototype.toJSON = function () { return "arr" }; var r = JSON.stringify({a: [1]}); delete Array.prototype.toJSON; r`,
);

// ciclos
add(
  `var a = {}; a.a = a; JSON.stringify(a)`, `var a = []; a.push(a); JSON.stringify(a)`, `var a = {b: {}}; a.b.c = a; JSON.stringify(a)`, `var a = {b: {c: []}}; a.b.c.push(a.b); JSON.stringify(a)`,
  `var a = {x: [{y: {}}]}; a.x[0].y.z = a.x; JSON.stringify(a)`, `var a = {}; a.b = {c: a}; JSON.stringify([a])`, `var a = {}; a.self = a; JSON.stringify({outer: a})`, `var a = {}; a.self = a; JSON.stringify(a, null, 2)`,
  `var a = {}; a.self = a; JSON.stringify(a, function (k, v) { return v })`, `var a = {}; a.self = a; JSON.stringify(a, ["self"])`, `var a = {}; a.self = a; JSON.stringify({toJSON() { return a }})`,
  `var a = {}, b = {a: a}; a.b = b; JSON.stringify(a)`, `var a = {}, b = {a: a}, c = {b: b}; a.c = c; JSON.stringify([c])`, `var a = [1]; a[1] = {k: a}; JSON.stringify(a)`,
  `var a = {}; a.x = [a]; JSON.stringify(a)`, `var a = {long_property_name_one: {long_property_name_two: {}}}; a.long_property_name_one.long_property_name_two.back = a; JSON.stringify(a)`,
  `var a = new Proxy({}, {get(t, k) { return a }, ownKeys() { return ["q"] }, getOwnPropertyDescriptor() { return {value: 1, enumerable: true, configurable: true} }}); JSON.stringify(a)`,
  `var s = {}; var t = {s: s, s2: s}; JSON.stringify(t)`, `var a = {}; JSON.stringify([a, a, a])`, `var a = {}; a.toJSON = function () { return a }; JSON.stringify(a)`,
  `var a = {}; a.x = {toJSON() { return a }}; JSON.stringify(a)`, `var m = new Map; m.set(1, m); JSON.stringify(m)`, `var a = {b: {}}; a.b.c = a; try { JSON.stringify(a) } catch (e) { e.message }`,
  `var a = {}; a.a = a; try { JSON.stringify(a) } catch (e) { e.name + "|" + e.constructor.name + "|" + e.message.length }`,
);

// objetos exóticos
add(
  `JSON.stringify(new Map([["a", 1]]))`, `JSON.stringify([new Set([1, 2])])`, `JSON.stringify(Object.fromEntries(new Map([["a", 1], ["b", [2]]])))`, `JSON.stringify({m: new Map, s: new Set, w: new WeakMap})`,
  `JSON.stringify(new Error("e"))`, `JSON.stringify(Object.assign(new Error("e"), {code: 5}))`, `JSON.stringify({e: new TypeError("t")})`, `JSON.stringify(/x/)`, `JSON.stringify(Object.assign(/x/, {k: 1}))`,
  `JSON.stringify(new Uint8Array([1, 2, 3]))`, `JSON.stringify(new Int16Array(2))`, `JSON.stringify(new Float64Array([0.5, -0, NaN]))`, `JSON.stringify(new BigInt64Array(1))`.replace("JSON.stringify(new BigInt64Array(1))", "typeof JSON.stringify(new Uint8Array(0))"),
  `JSON.stringify(new ArrayBuffer(2))`, `JSON.stringify(Symbol.for("x"))`, `JSON.stringify({[Symbol.toPrimitive]: 1})`, `JSON.stringify(Object(Symbol("q")))`,
  `JSON.stringify(function f() {})`, `JSON.stringify([function f() {}])`, `JSON.stringify({f() {}, a: 1})`, `JSON.stringify(class {})`, `JSON.stringify(async () => {})`, `JSON.stringify(function* () {}())`,
  `JSON.stringify(globalThis).length > 0`, `JSON.stringify(new (function F() { this.a = 1 }))`, `JSON.stringify(Object.create(Array.prototype))`, `JSON.stringify(Object.setPrototypeOf([1, 2], null))`,
  `JSON.stringify(Object.assign(Object.create(null), {a: 1}))`, `JSON.stringify(Reflect.ownKeys)`, `JSON.stringify(new Proxy({a: 1}, {ownKeys() { return ["a", "b"] }, getOwnPropertyDescriptor(t, k) { return k === "a" ? {value: 1, enumerable: true, configurable: true} : undefined }}))`,
  `JSON.stringify(new Proxy([1, [2]], {}))`, `JSON.stringify({a: new Proxy([], {get(t, k) { return k === "length" ? 2 : k }})})`, `var r = Proxy.revocable({}, {}); r.revoke(); JSON.stringify(r.proxy)`,
  `var r = Proxy.revocable([], {}); r.revoke(); JSON.stringify(r.proxy)`, `JSON.stringify([new Proxy(function () {}, {})])`,
  `JSON.stringify({length: 2, 0: "a", 1: "b"})`, `JSON.stringify(Object.assign([], {length: 3}))`, `var a = [1, 2, 3]; a.length = 10; JSON.stringify(a)`, `var a = []; a[5] = 1; JSON.stringify(a)`,
  `JSON.stringify({get a() { delete this.b; return 1 }, b: 2})`, `var o = {a: 1, b: 2}; JSON.stringify(o, function (k, v) { if (k === "a") delete o.b; return v })`,
  `JSON.stringify(Object.defineProperty([], "0", {get() { return "g" }, enumerable: true}))`, `JSON.stringify(Object.defineProperties({}, {a: {value: 1, enumerable: true}, b: {value: 2}}))`,
  `JSON.stringify("\\u{1F600}")`, `JSON.stringify("\\uD83D")`, `JSON.stringify("\\uDE00\\uD83D")`, `JSON.stringify({"\\uD83D": "\\uDE00"})`, `JSON.stringify(["\\ud800", "\\udbff", "\\udc00", "\\udfff", "\\ud800\\udc00"])`,
  `JSON.stringify("\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\u0008\\u000b\\u000e\\u000f\\u0010\\u001a\\u001b\\u001e\\u001f")`, `JSON.stringify("\\u007f\\u0080\\u009f\\u00a0\\u00ad\\u200b\\u2028\\u2029\\ufeff\\ufffe\\uffff")`,
  `JSON.stringify(Object.keys(JSON.parse(JSON.stringify({"\\ud800": 1}))).map(function (k) { return k.charCodeAt(0) }))`, `JSON.stringify("a".repeat(100000)).length`, `JSON.stringify(["x".repeat(5000)], null, 2).length`,
  `JSON.stringify({a: 1, b: 2}, null, "\\ud800")`, `JSON.stringify([[]], null, "ab")`, `JSON.stringify(1, null, 2)`, `JSON.stringify([1], null, "")`,
);

// profundidade em stringify
for (const n of [1000, 5000, 9000, 10000, 20000, 100000]) {
  add(`(function () { var a = []; for (var i = 0; i < ${n}; i++) a = [a]; return typeof JSON.stringify(a) })()`);
  add(`(function () { var a = {}; for (var i = 0; i < ${n}; i++) a = {a: a}; return typeof JSON.stringify(a) })()`);
  add(`(function () { var a = []; for (var i = 0; i < ${n}; i++) a = [a]; return typeof JSON.stringify(a, null, 1) })()`);
  add(`(function () { var a = []; for (var i = 0; i < ${n}; i++) a = [a]; return typeof JSON.stringify(a, function (k, v) { return v }) })()`);
}

// ---------------------------------------------------------------- Number.prototype.toString(radix)
const radixValues = [0, -0, 1, -1, 2, 10, 35, 36, 255, 256, 0.5, -0.5, 0.25, 0.1, 0.2, 0.3, 1 / 3, 2 / 3, 1 / 7, Math.PI, Math.E, Math.SQRT2, 1e21, 1e-7, 1e100, 1e300, -1e300, 5e-324, 2.2250738585072014e-308, 2.225073858507201e-308,
  1.7976931348623157e308, 2 ** 53, 2 ** 53 + 2, 2 ** 31, 2 ** 32, 2 ** 63, 2 ** 64, 2 ** 100, 2 ** -1, 2 ** -52, 2 ** -1022, 2 ** -1074, 123456789, 0.000001, 123.456, -123.456, 4294967295, 1e15, 1e16, 1e17, 0.9999999999999999, 1.0000000000000002,
  NaN, Infinity, -Infinity, 9007199254740991, 1.5, 255.255, 3.14159e10, 7.5e-9];
fillByHash(radixValues, 100, randomDouble);
fillByHash(radixValues, 200, randomSmall);
fillByHash(radixValues, 300, () => (rnd() < 0.5 ? -1 : 1) * Math.floor(rnd() * 2 ** 53) / 2 ** Math.floor(rnd() * 60));
for (const x of radixValues) {
  // A base (2 a 36) de cada valor sai do hash do literal.
  const radix = 2 + (parseInt(require("crypto").createHash("sha1").update("radix:" + lit(x)).digest("hex").slice(0, 8), 16) % 35);
  add(`(${lit(x)}).toString(${radix})`);
}
add(
  `(1).toString(1)`, `(1).toString(37)`, `(1).toString(0)`, `(1).toString(-1)`, `(1).toString(NaN)`, `(1).toString(Infinity)`, `(1).toString(1e10)`, `(1).toString("2")`, `(255).toString("16")`, `(255).toString(16.9)`, `(255).toString(null)`,
  `(255).toString(undefined)`, `(255).toString({valueOf() { return 8 }})`, `(255).toString(2n)`, `(255).toString(Symbol())`, `(255).toString("x")`, `(NaN).toString(1)`, `(NaN).toString(37)`, `(255).toString(36.99)`, `(255).toString(1.9)`,
  `Number.prototype.toString.call("1")`, `Number.prototype.toString.call({})`, `Number.prototype.toString.call(new Number(255), 16)`, `Number.prototype.toString.call(null)`, `Number.prototype.toString.length`,
  `(-255).toString(2)`, `(0.5).toString(2)`, `(-0).toString(2)`, `(1e21).toString(7)`, `(2 ** 70).toString(2).length`, `(1e300).toString(36).length`, `(5e-324).toString(2).length`, `(5e-324).toString(36)`, `(0.1).toString(3)`,
  `(1 / 3).toString(3)`, `(0.1).toString(2)`, `(0.1).toString(16)`, `(255.5).toString(16)`, `(1e21).toLocaleString === undefined`, `(2 ** 53).toString(36)`, `(35.99).toString(36)`, `(0.000001).toString(36)`,
);

// ---------------------------------------------------------------- toFixed / toPrecision / toExponential
const fmtValues = [0, -0, 1, -1, 0.5, 1.5, 2.5, -2.5, 0.05, 0.005, 0.0005, 1.005, 1.45, 8.345, 10.235, 1.255, 123.456, 0.1, 0.000001, 0.0000001, 1e20, 1e21, 1e-7, 123456789012345680000, 1.7976931348623157e308, 5e-324,
  NaN, Infinity, -Infinity, 999.9999, 0.9999, 99.5, 9.5, 0.45, 1e22, 2 ** 53, 25, 125, 1.25, 12345.6789, -0.0000005, 4.35, 0.615, 10.5, 1e-10, 3.14159265358979, 2 / 3, 1e100, 1.1e21, 0.3];
const digitsPool = [0, 1, 2, 3, 5, 10, 20, 21, 50, 99, 100, 101, -1, -100, 1.9, "3", "abc", undefined, null, NaN, Infinity, true, 1e10, {valueOf() { return 4 }}];
function digitLit(d) {
  if (typeof d === "object" && d !== null) return "{valueOf() { return 4 }}";
  if (typeof d === "string") return JSON.stringify(d);
  return String(d);
}
for (const method of ["toFixed", "toPrecision", "toExponential"]) {
  // Candidatos: todos os valores x todos os dígitos, mais valores gerados (pequenos e doubles) x dígitos comuns; 110 por hash.
  const fmtCandidates = [];
  for (const v of fmtValues) for (const d of digitsPool) fmtCandidates.push(`(${lit(v)}).${method}(${digitLit(d)})`);
  for (let n = 0; n < 60; n++) {
    const v = rnd() < 0.5 ? randomSmall() : randomDouble();
    const d = [0, 1, 2, 3, 4, 5, 6, 10, 15, 17, 20][Math.floor(rnd() * 11)];
    fmtCandidates.push(`(${lit(v)}).${method}(${digitLit(d)})`);
  }
  add(...sampleByHash(fmtCandidates, 110));
  add(`(1.5).${method}()`, `Number.prototype.${method}.call("1")`, `Number.prototype.${method}.call({}, 1)`, `Number.prototype.${method}.length`, `(123.456).${method}(3n)`, `(123.456).${method}(Symbol())`);
}
add(`(1000000000000000128).toFixed(0)`, `(1000000000000000128).toString()`, `(1e21).toFixed(2)`, `(0.5).toFixed(0)`, `(1.5).toFixed(0)`, `(2.5).toFixed(0)`, `(-1.5).toFixed(0)`, `(-0.0000001).toFixed(2)`, `(0).toFixed(100)`, `(1.1).toFixed(100)`,
  `(123.456).toPrecision(1)`, `(0.000123).toPrecision(2)`, `(123456).toPrecision(2)`, `(1e21).toPrecision(3)`, `(0).toPrecision(5)`, `(-0).toPrecision(5)`, `(0).toExponential(5)`, `(0).toExponential()`, `(123456).toExponential()`, `(0.00015).toExponential(1)`,
  `(NaN).toFixed(200)`, `(Infinity).toPrecision(0)`, `(Infinity).toExponential(-5)`, `(NaN).toExponential(500)`, `(1).toPrecision(0)`, `(1).toPrecision(101)`, `(1).toExponential(101)`, `(1).toFixed(101)`, `(1).toFixed(-0.5)`);

// ---------------------------------------------------------------- parseFloat / Number() / unary plus
const numStrings = [
  "", " ", "  \t\n  ", "0", "-0", "+0", "1", "-1", "+1", "1.5", ".5", "5.", "-.5", "+.5", ".", "-", "+", "+-1", "--1", "1e1", "1E1", "1e+1", "1e-1", "1e", "1e+", "1e-", ".5e-5", "5.e5", ".e5", "e5", "1e1000", "-1e1000", "1e-1000", "-1e-1000",
  "Infinity", "-Infinity", "+Infinity", "infinity", "INFINITY", "Infinityx", "Inf", "NaN", "nan", "-NaN", "0x", "0x0", "0x1f", "0X1F", "-0x1f", "+0x1f", "0xg", "0x1g", "0x1.8", "0x.8", "0xffffffffffffffff", "0x" + "f".repeat(300),
  "0o", "0o7", "0O17", "0o8", "-0o7", "0b", "0b1", "0B101", "0b2", "-0b1", "+0b1", "0b" + "1".repeat(60), "0o" + "7".repeat(30), "017", "08", "09.5", "00", "000.5", "-00", "0_1", "1_000", "1__0", "1_", "_1", "1_0.5", "1.5_5", "1e1_0", "0x1_f", "0b1_0", "0o7_7",
  " 1", "1 ", " 1 ", "\t1\n", "\u00a01", "1\u00a0", "\ufeff1", "1\ufeff", "\u20281", "\u20291", "\u30001", "\u20001", "\u200a1", "\u200b1", "1\u200b", "\u180e1", "\u205f1", "\u1680 1", "\u00851", "\u000b1", "\u000c1", "\u00001", "1\u0000",
  "1 2", "1,5", "1.5.5", "1e5e5", "1x", "x1", "1a", "0.5x", "1e5x", "1ee5", "12abc", "abc", "true", "false", "null", "undefined", "[]", "[1]", "{}", "1n", "1.5n", "0x1n",
  "123456789012345678901234567890", "0.1", "0.2", "0.30000000000000004", "9007199254740993", "9007199254740992.5", "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308", "1.797693134862316e308",
  "0." + "0".repeat(400) + "1", "1" + "0".repeat(400), "0." + "9".repeat(400), "1." + "0".repeat(400) + "1", "0.0000001", "123e-7", "00000.5e1", "5e-324", "2e-324", "3e-324", "1e308", "1.8e308", "1e-323", "123456789.123456789e5",
  "1e0000000000000000000005", "1e-0000000000000000000005", "1e999999999999999999999", "1e-999999999999999999999", "0e999999999999999999999", "0.0e-0", "-0e0", "-0.0",
  "１２３", "٣", "1\u0661", "１", "0x１", "1,000", "1 000", "$1", "1%", "(1)", "+ 1", "- 1", "1 e5", "1e 5", "1.e", "1.e1", "-.e1", "0.e0",
  "Infinity ", " Infinity", "-Infinity ", "- Infinity", "Infinit", "+Infinity5", "Infinity5", "infinity", "0xInfinity", "NaNN", "Number", "0b1.1", "0o1.5", "0b0", "0o0", "0x0.0", "00x1", "0xx1",
  "1e1.5", "1e+-1", "1e--1", "1e++1", "+1e1", "-1e-1", "-1e+1", ".1e1", "-.1e1", "+.1e1", "..1", "1..1", "1.1.", "0.1e", "e", "E1", ".E1", "1E", "1E+", "\n\n\n", "\r1\r", "1\r\n",
  "\u2028\u2029 1 \u2028", "\u00a0\u00a0", "\ufeff", "-\ufeff1", "\ufeff-1", "+\u00a01", "1\u00a0\u00a0", "0x\u00a01", "1\u202f", "\u202f1", "\u2003 1 \u2003", "\u3000\u3000-5.5\u3000",
];
// completa até 300 com combinações geradas
const pieces = ["", "0", "1", "9", "00", "0x", "0b", "0o", "+", "-", ".", "e", "E", "5", "e5", "_", " ", "\u00a0", "Infinity", "x1f", "12", "a"];
fillByHash(numStrings, 300, () => {
  let s = "";
  const n = 2 + Math.floor(rnd() * 4);
  for (let i = 0; i < n; i++) s += pieces[Math.floor(rnd() * pieces.length)];
  return s;
}, (text) => text);
for (const s of numStrings) {
  add(`parseFloat(${q(s)})`, `Number(${q(s)})`, `+${q(s)}`);
}
add(
  `parseFloat()`, `parseFloat(undefined)`, `parseFloat(null)`, `parseFloat(true)`, `parseFloat([1.5, 2])`, `parseFloat({toString() { return "7.5x" }})`, `parseFloat(1e21)`, `parseFloat(1e-7)`, `parseFloat(-0)`, `parseFloat(0.0000001)`, `parseFloat(Symbol())`, `parseFloat(1n)`,
  `Number()`, `Number(undefined)`, `Number(null)`, `Number(true)`, `Number([])`, `Number([5])`, `Number([1, 2])`, `Number({})`, `Number(new Date(5))`, `Number(Symbol())`, `Number(1n)`, `Number(2n ** 64n)`, `Number(2n ** 1024n)`, `Number(-(2n ** 1024n))`, `Number(2n ** 53n + 1n)`, `Number({valueOf() { return "3" }})`,
  `Number({valueOf() { return {} }, toString() { return "4" }})`, `Number({valueOf: 1, toString: 2})`, `Number("1", 2)`, `+[]`, `+{}`, `+null`, `+undefined`, `+true`, `+"  12  "`, `+1n`, `+Symbol()`, `+new Date(7)`, `-"5"`, `-"abc"`, `- -"5"`, `+"0x10" + +"0b10" + +"0o10"`,
  `Number.parseFloat === parseFloat`, `Number.parseInt === parseInt`, `parseFloat.length`, `Number.length`, `parseFloat.name`, `Number("1_000")`, `Number("0x")`, `Number("Infinity")`, `Number("-Infinity")`, `Number("1e1000")`,
  `parseInt("0x1f")`, `parseInt("  -0")`, `Object.is(parseInt("-0"), -0)`, `parseInt("1e3")`, `parseInt("12px")`, `parseInt("z", 36)`, `parseInt("10", 1)`, `parseInt("10", 37)`, `parseInt("9007199254740993")`, `parseInt("1" + "0".repeat(30))`, `parseInt("0.00000001")`, `parseInt(0.0000001)`, `parseInt(1e21)`,
);

// ---------------------------------------------------------------- BigInt(string), asIntN/asUintN
const bigStrings = [
  "", " ", "0", "-0", "+0", "1", "-1", "+1", "  12  ", "\n12\t", "1_000", "1n", "1.5", ".5", "1e3", "0x", "0x0", "0xff", "0XFF", "-0xff", "+0xff", "0b", "0b101", "0B101", "-0b1", "0o", "0o17", "0O17", "-0o17", "0o8", "0b2", "0xg",
  "12345678901234567890123456789012345678901234567890", "-12345678901234567890123456789012345678901234567890", "0x" + "f".repeat(100), "0b" + "1".repeat(200), "0o" + "7".repeat(80), "9007199254740993", "18446744073709551616", "-18446744073709551616",
  "Infinity", "NaN", "abc", "1 2", "1,2", "--1", "+-1", "-+1", "0.0", "1.", "1e", "00", "007", "-007", "0b0", "\u00a07\u00a0", "\ufeff7", "7\u2028", "\u200b7", "1__0", "0x_1", "٣", "１２",
  "1" + "0".repeat(500), "-" + "9".repeat(300), "0x8000000000000000", "0xffffffffffffffff", "0x10000000000000000", "-0x8000000000000000",
];
for (const s of bigStrings) add(`String(BigInt(${q(s)}))`, `typeof BigInt(${q(s)})`);
add(
  `BigInt(1)`, `BigInt(-1)`, `BigInt(0)`, `BigInt(-0)`, `BigInt(1.5)`, `BigInt(NaN)`, `BigInt(Infinity)`, `BigInt(1e21)`, `String(BigInt(1e21))`, `String(BigInt(2 ** 53))`, `String(BigInt(Number.MAX_VALUE))`.slice(0, -1) + ").length",
  `BigInt(true)`, `BigInt(false)`, `BigInt(null)`, `BigInt(undefined)`, `BigInt()`, `BigInt(Symbol())`, `BigInt({})`, `BigInt([])`, `BigInt([5])`, `BigInt({valueOf() { return 3 }})`, `BigInt({valueOf() { return 3.5 }})`, `BigInt({valueOf() { return "9" }})`,
  `BigInt(2n)`, `BigInt(Object(2n))`, `new BigInt(1)`, `BigInt.length`, `BigInt.name`, `BigInt(1, 2)`, `BigInt(Number.MAX_SAFE_INTEGER + 2)`, `BigInt(0.1)`, `BigInt(-0.5)`, `BigInt("9".repeat(100)) % 1000n`, `BigInt(new Date(5))`, `BigInt(1e300) > 0n`,
);
const bits = [0, 1, 2, 3, 8, 16, 31, 32, 33, 53, 63, 64, 65, 100, 128, 1000, "3", 1.9, -0, NaN, Infinity, -1, 2 ** 53, 2 ** 53 - 1, 2 ** 53 + 1, undefined, null, true, "x", 9007199254740991, 1e300];
const bigVals = ["0n", "1n", "-1n", "127n", "128n", "255n", "256n", "-128n", "-129n", "2n ** 63n", "2n ** 63n - 1n", "-(2n ** 63n)", "2n ** 64n", "2n ** 64n - 1n", "-(2n ** 64n)", "-(2n ** 64n) - 1n", "2n ** 100n + 5n", "-(2n ** 100n) + 5n", "12345678901234567890n", "-12345678901234567890n", "2n ** 1000n", "-(2n ** 1000n)", "(2n ** 53n) + 1n", "0xdeadbeefn", "-0xdeadbeefn"];
let k = 0;
for (const fn of ["asIntN", "asUintN"]) {
  // Todas as combinações de bits x valor são candidatas; 90 por hash do programa.
  const bigCandidates = [];
  for (const b of bits) for (const v of bigVals) bigCandidates.push(`BigInt.${fn}(${typeof b === "string" ? JSON.stringify(b) : lit(b)}, ${v})`);
  add(...sampleByHash(bigCandidates, 90));
  k++;
  add(`BigInt.${fn}(8, 1)`, `BigInt.${fn}(8)`, `BigInt.${fn}()`, `BigInt.${fn}(8, "3")`, `BigInt.${fn}(8, true)`, `BigInt.${fn}(8, {})`, `BigInt.${fn}(8, null)`, `BigInt.${fn}(8, Symbol())`, `BigInt.${fn}(8, undefined)`, `BigInt.${fn}(8, 1.5)`, `BigInt.${fn}(-1, 1n)`,
    `BigInt.${fn}(2 ** 53, 1n)`, `BigInt.${fn}(2 ** 53 - 1, 5n)`, `BigInt.${fn}(Symbol(), 1n)`, `BigInt.${fn}(1n, 1n)`, `BigInt.${fn}(0, 5n)`, `BigInt.${fn}.length`, `BigInt.${fn}(8, {valueOf() { return 300n }})`);
}
add(`(255n).toString(16)`, `(-255n).toString(2)`, `(255n).toString(36)`, `(255n).toString(1)`, `(255n).toString(37)`, `(255n).toString()`, `(2n ** 200n).toString(36)`, `BigInt.prototype.toString.call(1)`, `(1n).toString(undefined)`, `(0n).toString(2)`, `(-0n).toString()`, `(10n ** 40n).toString(7)`);

// ---------------------------------------------------------------- String(num): menor representação que reconverte
const stringNums = [];
const base = [0, -0, 1, -1, 0.1, 0.2, 0.3, 0.1 + 0.2, 1 / 3, 2 / 3, 100, 1e5, 1e20, 1e21, 1.5e21, 123456789012345680000, 1e-5, 1e-6, 1e-7, 1.5e-7, 123e-20, 5e-324, 1.5e-323, 2.2250738585072014e-308, 1.7976931348623157e308, 2 ** 53, 2 ** 53 + 2, 2 ** 64, 2 ** 70, 2 ** 100,
  9007199254740993, 4.35, 0.000001, 0.0000001, 1e300, 1.0000000000000002, 0.9999999999999999, 1.7976931348623155e308, 4.9e-324, 9.5367431640625e-7, 123456.789, 0.5, 1e23, 8.41e21, 1e22, 5e-7, 4.35e-7, 100000000000000000000, 999999999999999900000, 2e21, 33.33333333333333, 0.30000000000000004, 1.2345e-8, 6.02214076e23];
for (const x of base) stringNums.push(x);
fillByHash(stringNums, 120, randomDouble);
fillByHash(stringNums, 160, randomSmall);
fillByHash(stringNums, 200, () => Math.floor(rnd() * 2 ** 53) * 2 ** (Math.floor(rnd() * 80) - 40));
for (const x of stringNums) {
  add(`String(${lit(x)})`, `String(-${lit(x).replace(/^-/, "")})`);
}
add(`String(NaN)`, `String(Infinity)`, `String(-Infinity)`, `(1e21).toString()`, `(-1e21).toString()`, `(1e21 + 1).toString()`, `1e21 + ""`, `-0 + ""`, `` + "`${-0}`" + ``, `[-0] + ""`, `String([1e21, 1e-7])`, `String(0.1 * 3)`, `String(5e-324 / 2)`, `String(2 ** -1075)`);

// ---------------------------------------------------------------- saída
const out = [];
for (const source of programs) {
  if (/[\t\n\r]/.test(source)) throw new Error("programa com quebra de linha ou tab: " + source.slice(0, 80));
  const result = harness(source);
  if (/[\t\n\r]/.test(result.replace(/\t/g, ""))) throw new Error("resultado com quebra de linha: " + source.slice(0, 80));
  out.push(source + "\t" + result);
}
process.stdout.write(require("./golden-prelude.js").assertPublicResult(out.join("\n") + "\n"));
process.stderr.write(programs.length + " programas\n");
