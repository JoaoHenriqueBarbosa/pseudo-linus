// Gera tests/golden/key_order_exotic_bun.tsv: ordem das chaves próprias (Reflect.ownKeys, Object.keys, for-in,
// JSON.stringify, Object.assign, spread, Object.entries, getOwnPropertyNames) de objetos exóticos depois de sequências de
// operações: arrays com índices além de length e índices 2**32-2 / 2**32-1, strings boxed, typed arrays com chaves
// numéricas canônicas e não canônicas, arguments mapeado e não mapeado, funções e classes (length, name, prototype após
// delete e redefinição), Error, RegExp, objetos com chaves inteiras grandes e pequenas, delete e re-add, defineProperty
// de índice em ordem decrescente e Proxy sem trap. Medido no bun 1.4.2; cada programa roda num bun filho novo (no
// máximo 6 em paralelo, timeout de 8 s), e o resultado é o texto da global `R`.
// Formato fatorado (scripts/golden-prelude.js); dedup contra os goldens vizinhos por knownPrograms.
// Uso: bun scripts/gen-key-order-exotic-golden.js
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");
const { knownPrograms, emitFactored, sampleByHash, GOLDEN_DIR } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

const NAME = "key_order_exotic";
const PRELUDE = [
  "function ks(a) { return '[' + a.map(function (k) { return typeof k === 'symbol' ? k.toString() : String(k); }).join(',') + ']'; }",
  "function sv(v) {",
  "  if (typeof v === 'function') return 'fn';",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (v !== null && typeof v === 'object') return 'obj';",
  "  return Object.is(v, -0) ? '-0' : String(v);",
  "}",
  "function fi(o) { var r = []; for (var k in o) r.push(k); return r; }",
  "function J(o) { try { return String(JSON.stringify(o)); } catch (e) { return 'E:' + e.name; } }",
  "var OBS = {",
  "  own: function (o) { return ks(Reflect.ownKeys(o)); },",
  "  keys: function (o) { return ks(Object.keys(o)); },",
  "  forin: function (o) { return ks(fi(o)); },",
  "  json: function (o) { return J(o); },",
  "  assign: function (o) { return ks(Reflect.ownKeys(Object.assign({}, o))); },",
  "  spread: function (o) { return ks(Reflect.ownKeys({ ...o })); },",
  "  entries: function (o) { return Object.entries(o).map(function (e) { return e[0] + '=' + sv(e[1]); }).join(','); },",
  "  gopn: function (o) { return ks(Object.getOwnPropertyNames(o)); },",
  "};",
  "OBS.all = function (o) { return ['own', 'keys', 'forin', 'json', 'assign', 'spread', 'entries', 'gopn'].map(function (n) { return OBS[n](o); }).join(' | '); };",
  "OBS.nojson = function (o) { return ['own', 'keys', 'forin', 'assign', 'spread', 'gopn'].map(function (n) { return OBS[n](o); }).join(' | '); };",
  "function T(fn) { try { globalThis.R = String(fn()); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }",
  "",
].join("\n");

const DASHES = new RegExp("[" + String.fromCharCode(0x2013, 0x2014) + "]"); // varre o resultado
const data = (key) => `Object.defineProperty(o, ${key}, { value: 1, enumerable: true, configurable: true, writable: true })`;

const BASES = [
  // arrays
  "[]", "[1, 2, 3]", "[, 1, , 3]", "new Array(5)", "[1, 2, 3, 4, 5, 6]", "Object.assign([1, 2], { x: 1, 7: 7 })",
  // strings boxed
  'new String("abc")', 'new String("")', 'Object.assign(new String("ab"), { z: 1, 5: 5, y: 2 })', 'new String("\\ud83d\\ude00x")',
  // typed arrays
  "new Uint8Array(3)", "new Float64Array(2)", "new Uint8Array(0)", "new Int16Array([1, 2, 3, 4])", "Object.assign(new Uint8Array(2), { x: 1 })", "new BigInt64Array(2)",
  // arguments
  "(function (a, b) { return arguments; })(1, 2, 3)", '(function (a) { "use strict"; return arguments; })(1, 2)', "(function (a, b) { return arguments; })()",
  "(function (a, b) { return arguments; })(1)", "(function () { return arguments; })()", "(function (a, ...r) { return arguments; })(1, 2, 3)", "(function (a = 1, b) { return arguments; })(1, 2)",
  // funções e classes
  "function f(a, b) {}", "(function () {})", "(function named(a) {})", "((a, b, c) => 1)", "(async function af(x) {})", "(function* g() {})", "(function f(a) {}).bind(null, 1)",
  "(class C { static s = 1; static m() {} x() {} })", "(class { static a = 1; static b = 2; })", "(class D extends Array { static q = 1; })", "(class { static get g() { return 1; } static set g(v) {} })",
  "(class E { constructor(a, b) {} static name2 = 1; })", "(class { static name() {} })", "(class { static length = 3; })",
  "(class P {}).prototype", "(class Q { m() {} get g() { return 1; } static s() {} }).prototype", "(function F() {}).prototype",
  // Error, RegExp e outros
  'new Error("m")', 'new Error("m", { cause: 1 })', 'new TypeError("t")', 'new AggregateError([1], "a")', 'Object.assign(new Error("m"), { z: 1, 3: 3 })',
  "/a/g", 'new RegExp("a", "y")', "/b/.exec('b')", "'xay'.match(/a(?<n>y)/)", "new Date(0)", "new Map()", "Object(1)", "Object(Symbol.iterator)", "Object(1n)",
  // objetos comuns
  "{}", "{ b: 1, a: 2, 2: 1, 1: 1 }", "{ 4294967294: 1, 4294967295: 1, 5: 1, a: 1 }", "{ '-0': 1, '1.0': 1, '1e3': 1, 0: 1 }",
  "{ 3: 1, a: 1, [Symbol.for('s')]: 1, 1: 1 }", "Object.create({ inh: 1, 9: 1 })", "Object.create({ inh: 1 }, { own: { value: 1, enumerable: true } })",
  // proxies sem trap ownKeys
  "new Proxy({ b: 1, 2: 1, a: 2, 1: 1 }, {})", "new Proxy([1, 2, 3], {})", "new Proxy(function pf(a) {}, {})", "new Proxy(new Uint8Array(2), {})",
  "new Proxy(new String('ab'), {})", "new Proxy((function (a) { return arguments; })(1, 2), {})", "new Proxy({}, { get: function () { return 1; } })", "new Proxy(new Proxy({ x: 1, 3: 1 }, {}), {})",
];

const KEYS = ['"10"', '"4294967294"', '"4294967295"', '"4294967296"', '"-0"', '"1.0"', '"1e3"', '"01"', '"-1"', '"0"', '"1"', '"2"', '"3"', '"x"', '"length"', '"name"', '"prototype"', '"message"', '"lastIndex"', '"cause"', '"stack"', '"callee"', '"9007199254740991"', '"1.5"', '"NaN"', '"Infinity"', "Symbol.for('s')", "Symbol.iterator"];
const OPS = [];
for (const k of KEYS) {
  OPS.push(`o[${k}] = 1`, `delete o[${k}]`, data(k));
}
OPS.push(
  "o.x = 1", "o.y = 2", "o.b = 1", "o.a = 1", "o[5] = 1", "o[0] = 1", "o[7] = 7", "o[100] = 1", "o[3] = 1; delete o[3]; o[3] = 2", "o.x = 1; delete o.x; o.x = 2",
  "o.length = 0", "o.length = 2", "o.length = 10", "delete o.length", "delete o.name", "delete o.prototype", "delete o.message", "delete o.stack", "delete o.lastIndex", "delete o.callee",
  'Object.defineProperty(o, "length", { value: 4 })', 'Object.defineProperty(o, "name", { value: "z" })', 'Object.defineProperty(o, "prototype", { value: {} })',
  'Object.defineProperty(o, "name", { value: "z", enumerable: true })', 'Object.defineProperty(o, "length", { enumerable: true })',
  'Object.defineProperty(o, "prototype", { enumerable: true, value: 1, writable: true })', 'Object.defineProperty(o, "message", { enumerable: true, value: "q" })',
  'Object.defineProperty(o, "lastIndex", { enumerable: true, value: 3 })', 'Object.defineProperty(o, "stack", { enumerable: true, value: 1 })', 'Object.defineProperty(o, "callee", { enumerable: true, value: 1 })',
  'delete o.name; o.name = "n"', 'delete o.length; o.length = 1', 'delete o.prototype; o.prototype = {}', 'delete o.name; Object.defineProperty(o, "name", { value: 1, enumerable: true, configurable: true })',
  "o.lastIndex = 3", "o.message = 'k'", "o.cause = 1", "o.stack = 's'",
  "for (var i = 20; i > 0; i -= 3) Object.defineProperty(o, i, { value: i, enumerable: true, configurable: true })",
  "for (var i = 0; i < 12; i++) o[i * 1000003] = i",
  "for (var i = 6; i >= 0; i--) o[i * 100] = i",
  "for (var i = 0; i < 6; i++) o['k' + i] = i",
  "for (var i = 5; i >= 0; i--) o['k' + i] = i; for (var j = 0; j < 3; j++) o[j] = j",
  "o[4294967294] = 1; o[4294967295] = 2; o[4294967293] = 3",
  "o[4294967295] = 2; o[4294967294] = 1",
  "o[3] = 1; o[2] = 1; o[1] = 1; o[0] = 1",
  "o[1] = 1; o.a = 1; o[0] = 1; o.b = 1",
  "o[Symbol.for('s')] = 1; o.q = 1; o[2] = 1",
  "delete o[0]; o[0] = 1", "delete o[1]; o[1] = 5; delete o[2]", "for (var k of Reflect.ownKeys(o)) delete o[k]", "for (var k of Object.keys(o)) delete o[k]; o.z = 1",
  "Object.freeze(o)", "Object.seal(o)", "Object.preventExtensions(o)", "Object.setPrototypeOf(o, { p: 1, 4: 1 })",
  "Object.defineProperty(o, 'h', { value: 1, enumerable: false }); o.v = 1", "Object.defineProperty(o, 5, { get: function () { return 1; }, enumerable: true, configurable: true })",
  "Object.defineProperty(o, 'g', { get: function () { return 1; }, enumerable: true, configurable: true })", "Object.assign(o, { 8: 1, q: 2, 1: 3 })", "Object.assign(o, [9, 8, 7])", "Object.assign(o, 'xy')",
  "Object.defineProperties(o, { 9: { value: 1, enumerable: true, configurable: true }, 3: { value: 1, enumerable: true, configurable: true }, w: { value: 1, enumerable: true, configurable: true } })",
  "Array.prototype.push.call(o, 1, 2)", "Array.prototype.splice.call(o, 0, 1)", "Array.prototype.reverse.call(o)", "Array.prototype.sort.call(o)", "Array.prototype.fill.call(o, 0)", "Array.prototype.shift.call(o)", "Array.prototype.unshift.call(o, 'u')",
  "Array.prototype.pop.call(o)", "Array.prototype.copyWithin.call(o, 0, 1)",
  "if (o.fill) o.fill(1)", "if (o.set) o.set([5, 6])", "if (o.reverse) o.reverse()", "if (o.sort) o.sort()", "o.exec && o.exec('a')", "o.test && o.test('a')",
  "Object.defineProperty(o, 'length', { value: 0 }); o[2] = 1", "o.length = 0; o[0] = 1; o.length = 3",
);

const OBSERVERS = ["all", "own", "keys", "forin", "json", "assign", "spread", "entries", "gopn"];

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = mulberry32(20261008);
const pick = (list) => list[Math.floor(rand() * list.length)];

function program(base, ops, observer) {
  const body = ops.map((op) => `try { ${op}; } catch (e) {}`).join(" ");
  // Índice 2**32-2 num array dá length 4294967295 e o JSON.stringify percorreria tudo.
  const heavy = /4294967294|4294967295|4294967293/.test(ops.join(" "));
  const name = heavy && (observer === "all" || observer === "json") ? "nojson" : observer;
  return `T(function () { var o = ${base}; ${body} return OBS.${name}(o); });\n`;
}

const bodies = [];
const seen = new Set(knownPrograms("key_order_exotic_bun.tsv", (name) => name !== `${NAME}_bun.tsv`));
const add = (suffix) => {
  const full = PRELUDE + suffix;
  if (seen.has(full) || seen.has(suffix) || usesHostApi(suffix)) return;
  seen.add(full);
  bodies.push(suffix);
};

// 1) cada base com cada operação isolada, observador "all".
for (const base of BASES) for (const op of OPS) add(program(base, [op], "all"));
// 2) pares e trincas, com observador variado. O PRNG só GERA um conjunto de candidatos três vezes maior que o golden; quem
// escolhe os 4200 é o `sampleByHash`, pelo texto do programa, antes de descontar os goldens vizinhos (feito em `add`).
const randomCandidates = new Set();
for (let n = 0; n < 4200 * 3; n++) {
  const count = 2 + Math.floor(rand() * 3);
  const ops = [];
  for (let i = 0; i < count; i++) ops.push(pick(OPS));
  randomCandidates.add(program(pick(BASES), ops, pick(OBSERVERS)));
}
for (const suffix of sampleByHash([...randomCandidates], 4200)) add(suffix);
process.stderr.write(`programas candidatos: ${bodies.length}\n`);

// ---- Execução: um bun filho por programa, 6 em paralelo, timeout de 8 s.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "key-order-golden-"));
const preload = path.join(dir, "preload.js");
fs.writeFileSync(preload, "process.on('exit', () => { require('fs').writeSync(1, '\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n");

function runOne(slot, suffix) {
  return new Promise((resolve) => {
    const sourceFile = path.join(dir, `src${slot}.js`);
    const file = path.join(dir, `case${slot}.js`);
    fs.writeFileSync(sourceFile, PRELUDE + suffix);
    fs.writeFileSync(file, `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(sourceFile)}, "utf8")) } catch (e) {}\n`);
    const child = spawn(process.execPath, ["--preload", preload, file], { cwd: dir, stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    child.stdout.on("data", (chunk) => (out += chunk));
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.on("close", () => {
      clearTimeout(timer);
      const marked = out.split("\n").find((line) => line.startsWith("\u0001"));
      let parsed = null;
      try {
        parsed = marked ? JSON.parse(marked.slice(1)) : null;
      } catch (error) {
        parsed = null;
      }
      resolve(parsed);
    });
    child.on("error", () => {
      clearTimeout(timer);
      resolve(null);
    });
  });
}

async function main() {
  const rows = new Array(bodies.length).fill(null);
  let next = 0;
  async function worker(slot) {
    while (next < bodies.length) {
      const index = next++;
      rows[index] = await runOne(slot, bodies[index]);
    }
  }
  await Promise.all([0, 1, 2, 3, 4, 5].map(worker));
  const kept = [];
  let dropped = 0;
  const hostPath = /\/home\/|\/tmp\/|\/Users\//;
  bodies.forEach((suffix, index) => {
    const result = rows[index];
    if (result === null || result.length > 5000 || hostPath.test(result) || result.includes(dir) || DASHES.test(result)) {
      dropped++;
      return;
    }
    kept.push({ source: PRELUDE + suffix, result });
  });
  fs.writeFileSync(path.join(GOLDEN_DIR, `${NAME}_bun.tsv`), emitFactored(NAME, kept));
  process.stderr.write(`mantidos ${kept.length}, descartados ${dropped}\n`);
  fs.rmSync(dir, { recursive: true, force: true });
}
main();
