// Gera tests/golden/coercion_bun.tsv (e tests/golden/coercion_prelude.js): coerção e operadores medidos no bun 1.4.2.
// A matriz é de 60 valores (índices 0..59, fábrica `mk(i)` que devolve um valor novo a cada chamada) contra os
// operadores binários e unários, amostrada de forma determinística (LCG com semente fixa) por grupo de operadores.
// Colunas: a expressão (texto puro, usa `mk` e `Pair`) e o resultado serializado (JSON de uma string). O resultado é
// `ser(valor)` (tipo e valor, `-0` e `NaN` distintos) ou `error Nome: mensagem`. O prelúdio vive em
// `tests/golden/coercion_prelude.js` e o teste Rust o põe na frente de cada programa.
// Uso: timeout 120 bun scripts/gen-coercion-golden.js > tests/golden/coercion_bun.tsv
const fs = require("fs");
const path = require("path");

// O golden tem datas locais: o fuso é fixo para o oráculo não depender da máquina (o teste Rust fixa o mesmo).
process.env.TZ = "America/Sao_Paulo";

const values = [
  "undefined", "null", "true", "false", "0", "-0", "1", "-1", "NaN", "Infinity",
  "-Infinity", "2 ** 31", "2 ** 32", "2 ** 53", "1e21", "1e-7", "0.1", "''", "' '", "'0'",
  "'1'", "'1e3'", "'0x10'", "'0b11'", "'-0'", "'abc'", "'  42  '", "'\\n'", "[]", "[0]",
  "[1, 2]", "{}", "{ valueOf() { return 7 } }", "{ toString() { return '8' } }", "{ valueOf: null, toString() { return 'x' } }", "Symbol()", "1n", "-1n",
  "0n", "2n ** 64n", "function () {}", "new Date(0)", "new String('s')", "new Number(3)", "new Boolean(false)",
  "{ [Symbol.toPrimitive](hint) { return hint === 'default' ? 10 : hint === 'number' ? 20 : 30 } }",
  "{ [Symbol.toPrimitive](hint) { return hint } }",
  "{ valueOf() { throw new RangeError('boom') } }",
  "new Proxy({}, {})",
  "Object.create(null)",
  "new Proxy(function () {}, {})",
  "-(2 ** 31)", "0.5", "-1.5", "Number.MAX_VALUE", "Number.MIN_VALUE",
  "'9007199254740993'", "[[]]", "[null]",
  "{ [Symbol.toPrimitive]() { return {} } }",
];
if (values.length !== 60) throw new Error("esperava 60 valores, vieram " + values.length);

const prelude = `// Prelúdio do golden de coerção (gerado por scripts/gen-coercion-golden.js).
class Pair { constructor(result, after) { this.result = result; this.after = after; } }
const F = [
${values.map((source) => `  () => (${source}),`).join("\n")}
];
function mk(index) { return F[index](); }
function ser(v) {
  if (v instanceof Pair) return ser(v.result) + " | " + ser(v.after);
  switch (typeof v) {
    case "undefined": return "undefined";
    case "boolean": return "boolean " + v;
    case "number": return "number " + (Object.is(v, -0) ? "-0" : String(v));
    case "string": return "string " + JSON.stringify(v);
    case "bigint": return "bigint " + String(v);
    case "symbol": return "symbol " + Symbol.prototype.toString.call(v);
    case "function": return "function";
    default:
      if (v === null) return "null";
      if (Array.isArray(v)) return "array " + v.length;
      return "object " + Object.prototype.toString.call(v);
  }
}
`;

// Roda um programa no escopo global do próprio bun, igual ao que o teste Rust faz no motor.
function run(expr) {
  globalThis.R = undefined;
  const program = prelude + "\nvar R; try { R = ser(" + expr + "); } catch (e) { R = 'error ' + (e && e.name) + ': ' + (e && e.message); }";
  (0, eval)(program);
  return globalThis.R;
}

// Embaralha de forma determinística (LCG de 32 bits) e devolve os primeiros `count` pares de 60x60.
function samplePairs(seed, count) {
  const all = [];
  for (let a = 0; a < 60; a++) for (let b = 0; b < 60; b++) all.push([a, b]);
  let state = seed >>> 0;
  const next = () => (state = (Math.imul(state, 1664525) + 1013904223) >>> 0);
  for (let i = all.length - 1; i > 0; i--) {
    const j = next() % (i + 1);
    [all[i], all[j]] = [all[j], all[i]];
  }
  return all.slice(0, count);
}

const groups = [
  { ops: ["+", "-", "*", "/", "%", "**"], count: 700 },
  { ops: ["<", ">", "<=", ">="], count: 500 },
  { ops: ["==", "!=", "===", "!=="], count: 400 },
  { ops: ["&", "|", "^", "<<", ">>", ">>>"], count: 350 },
  { ops: ["in", "instanceof"], count: 300 },
  { ops: ["??", "&&", "||"], count: 150 },
];

const expressions = [];
let seed = 20261008;
for (const group of groups) {
  for (const op of group.ops) {
    for (const [a, b] of samplePairs(seed++, group.count)) expressions.push(`mk(${a}) ${op} mk(${b})`);
  }
}
for (let a = 0; a < 60; a++) {
  expressions.push(`+mk(${a})`, `-mk(${a})`, `~mk(${a})`, `!mk(${a})`, `typeof mk(${a})`, `void mk(${a})`);
  expressions.push(`delete mk(${a}).x`);
  for (const form of ["x++", "x--", "++x", "--x"]) {
    expressions.push(`(function () { var x = mk(${a}); var r = ${form}; return new Pair(r, x); })()`);
  }
}

fs.writeFileSync(path.join(__dirname, "..", "tests", "golden", "coercion_prelude.js"), prelude);
const rows = [];
for (const expr of expressions) {
  if (/[\t\n]/.test(expr)) throw new Error("expressão com tab ou quebra de linha: " + expr);
  const result = run(expr);
  if (/\/home\/|\/tmp\/|\/Users\//.test(result)) throw new Error("caminho da máquina no resultado: " + expr);
  rows.push(expr + "\t" + JSON.stringify(result));
}
process.stdout.write(require("./golden-prelude.js").assertPublicResult(rows.join("\n") + "\n"));
