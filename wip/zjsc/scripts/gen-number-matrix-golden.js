// Gera tests/golden/number_matrix_bun.tsv: matriz de Intl.NumberFormat#formatToParts, #formatRange e #formatRangeToParts
// em 12 locales, medida no bun 1.4.2. Combina style (decimal, percent, currency, unit), notation (standard, compact,
// scientific, engineering), compactDisplay, signDisplay (incluindo negative), currencyDisplay e currencySign, useGrouping
// (todos os valores), roundingMode, roundingIncrement, roundingPriority, trailingZeroDisplay e dígitos significativos,
// com valores extremos (0, -0, NaN, Infinity, 1e21, BigInt e strings decimais longas).
// Programas cujo `new Intl.NumberFormat(...).método(...)` já aparece nos outros goldens de número ou de Intl são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num processo bun filho novo, sem APIs de host.
// Uso: bun scripts/gen-number-matrix-golden.js > tests/golden/number_matrix_bun.tsv
const { emitFactoredLines, sampleByHash } = require("./golden-prelude.js");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function P(a){return a.map(function(x){return x.type+":"+x.value+(x.source?":"+x.source:"")}).join("|")}\n' +
  'function T(f){try{return f()}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const LOCALES = ["en-US", "pt-BR", "es-ES", "fr-FR", "de-DE", "ja-JP", "ko-KR", "zh-CN", "ru-RU", "tr-TR", "ar-AE", "sv-SE"];

const VALUES = [
  "0", "-0", "NaN", "Infinity", "-Infinity", "1e21", "-1e21", "123456789n", "-98765432109876543210n", "0n",
  "'1234567890.123456789'", "'-0.000001234'", "'99999999999999999999.99'", "0.5", "-1234.5", "1e-7", "999999.995", "0.000123", "42", "-7.25",
];

const o = (x) => JSON.stringify(x);
const OPTION_SETS = [];
const addOpts = (...list) => OPTION_SETS.push(...list);

addOpts({});
for (const signDisplay of ["auto", "always", "exceptZero", "negative", "never"]) {
  addOpts({ signDisplay }, { style: "percent", signDisplay }, { style: "currency", currency: "USD", signDisplay },
    { style: "currency", currency: "EUR", currencySign: "accounting", signDisplay }, { notation: "compact", signDisplay },
    { notation: "scientific", signDisplay });
}
for (const currencyDisplay of ["symbol", "narrowSymbol", "code", "name"]) {
  addOpts({ style: "currency", currency: "USD", currencyDisplay }, { style: "currency", currency: "BRL", currencyDisplay },
    { style: "currency", currency: "JPY", currencyDisplay, currencySign: "accounting" },
    { style: "currency", currency: "EUR", currencyDisplay, notation: "compact" });
}
for (const unit of ["kilometer", "celsius", "megabyte", "kilometer-per-hour", "liter", "percent", "day"]) {
  for (const unitDisplay of ["short", "long", "narrow"]) addOpts({ style: "unit", unit, unitDisplay });
}
addOpts({ style: "unit", unit: "meter", notation: "compact", compactDisplay: "long" },
  { style: "unit", unit: "second", signDisplay: "always", unitDisplay: "long" });
for (const notation of ["standard", "scientific", "engineering"]) {
  addOpts({ notation }, { notation, maximumFractionDigits: 1 }, { notation, minimumFractionDigits: 3 },
    { notation, maximumSignificantDigits: 3 });
}
for (const compactDisplay of ["short", "long"]) {
  addOpts({ notation: "compact", compactDisplay }, { notation: "compact", compactDisplay, style: "currency", currency: "USD" },
    { notation: "compact", compactDisplay, minimumFractionDigits: 1 }, { notation: "compact", compactDisplay, maximumSignificantDigits: 2 },
    { notation: "compact", compactDisplay, style: "percent" });
}
for (const useGrouping of [true, false, "min2", "always", "auto", "true", "false"]) {
  addOpts({ useGrouping }, { useGrouping, style: "currency", currency: "EUR" }, { useGrouping, notation: "compact" });
}
for (const roundingMode of ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"]) {
  addOpts({ roundingMode, maximumFractionDigits: 0 }, { roundingMode, maximumFractionDigits: 2, minimumFractionDigits: 2 },
    { roundingMode, maximumSignificantDigits: 2 });
}
for (const roundingIncrement of [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000, 5000]) {
  addOpts({ roundingIncrement, minimumFractionDigits: 2, maximumFractionDigits: 2 },
    { roundingIncrement, minimumFractionDigits: 0, maximumFractionDigits: 0 },
    { roundingIncrement, roundingMode: "ceil", minimumFractionDigits: 2, maximumFractionDigits: 2 });
}
addOpts({ roundingIncrement: 3 }, { roundingIncrement: 5, maximumSignificantDigits: 3 }, { roundingIncrement: 5, roundingPriority: "morePrecision" });
for (const roundingPriority of ["auto", "morePrecision", "lessPrecision"]) {
  addOpts({ roundingPriority, maximumFractionDigits: 1, maximumSignificantDigits: 3 },
    { roundingPriority, minimumFractionDigits: 2, maximumFractionDigits: 4, minimumSignificantDigits: 2, maximumSignificantDigits: 5 },
    { roundingPriority, maximumFractionDigits: 0, maximumSignificantDigits: 2, trailingZeroDisplay: "stripIfInteger" });
}
for (const trailingZeroDisplay of ["auto", "stripIfInteger"]) {
  addOpts({ trailingZeroDisplay, minimumFractionDigits: 2 }, { trailingZeroDisplay, style: "currency", currency: "USD" },
    { trailingZeroDisplay, minimumSignificantDigits: 4 }, { trailingZeroDisplay, notation: "compact", minimumFractionDigits: 1 },
    { trailingZeroDisplay, style: "percent", minimumFractionDigits: 1 });
}
for (const [min, max] of [[1, 1], [1, 3], [3, 3], [2, 5], [1, 21], [5, 10]]) {
  addOpts({ minimumSignificantDigits: min, maximumSignificantDigits: max },
    { minimumSignificantDigits: min, maximumSignificantDigits: max, style: "currency", currency: "USD" },
    { minimumSignificantDigits: min, maximumSignificantDigits: max, notation: "scientific" });
}
addOpts({ minimumIntegerDigits: 3 }, { minimumIntegerDigits: 21, minimumFractionDigits: 2 }, { maximumFractionDigits: 100 },
  { minimumFractionDigits: 20, maximumFractionDigits: 20 }, { maximumSignificantDigits: 0 }, { notation: "bogus" });

// Os conjuntos de opções que valem para formatRange (menos, para manter o custo).
const RANGE_OPTION_SETS = sampleByHash(OPTION_SETS, Math.ceil(OPTION_SETS.length / 3), (opts) => o(opts));
const RANGE_PAIRS = [
  ["1", "2"], ["-5", "5"], ["0", "0"], ["1e21", "2e21"], ["1.5", "1.5"], ["1000", "1000000"], ["0.001", "0.002"], ["-0", "0"],
  ["5n", "99999999999999999999n"], ["'1234567890.123456789'", "'1234567890.123456790'"], ["Infinity", "Infinity"], ["-Infinity", "Infinity"],
  ["999.5", "1000.5"], ["3", "1"], ["0.99", "1.01"], ["1e-7", "5e-7"],
];

// Cada família entra como o conjunto de todas as combinações (locale x opções x valor ou par) e a amostra sai por hash do
// texto do programa (`sampleByHash`), na quantidade que a grade antiga escolhia por posição: 4 valores por (locale, opções),
// 3 pares de formatRange por (locale, opções de intervalo) e 2 de formatRangeToParts para metade delas.
const exprs = [];
const toPartsCandidates = [];
const rangeCandidates = [];
const rangePartsCandidates = [];
for (const locale of LOCALES) {
  for (const opts of OPTION_SETS) {
    for (const value of VALUES) toPartsCandidates.push(`T(()=>P(new Intl.NumberFormat(${o(locale)},${o(opts)}).formatToParts(${value})))`);
  }
  for (const opts of RANGE_OPTION_SETS) {
    for (const [a, b] of RANGE_PAIRS) {
      rangeCandidates.push(`T(()=>new Intl.NumberFormat(${o(locale)},${o(opts)}).formatRange(${a},${b}))`);
      rangePartsCandidates.push(`T(()=>P(new Intl.NumberFormat(${o(locale)},${o(opts)}).formatRangeToParts(${a},${b})))`);
    }
  }
}
exprs.push(...sampleByHash(toPartsCandidates, LOCALES.length * OPTION_SETS.length * 4));
exprs.push(...sampleByHash(rangeCandidates, LOCALES.length * RANGE_OPTION_SETS.length * 3));
exprs.push(...sampleByHash(rangePartsCandidates, LOCALES.length * Math.ceil(RANGE_OPTION_SETS.length / 2) * 2));
// Argumentos inválidos e coerção de formatRange.
for (const locale of LOCALES) {
  exprs.push(
    `T(()=>new Intl.NumberFormat(${o(locale)}).formatRange(undefined,1))`, `T(()=>new Intl.NumberFormat(${o(locale)}).formatRange(1,undefined))`,
    `T(()=>new Intl.NumberFormat(${o(locale)}).formatRange(NaN,1))`, `T(()=>new Intl.NumberFormat(${o(locale)}).formatRange(1,NaN))`,
    `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatRangeToParts('x',1)))`, `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatToParts('0x1F')))`,
    `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatToParts('  12  ')))`, `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatToParts('')))`,
    `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatToParts(null)))`, `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatToParts(true)))`,
    `T(()=>P(new Intl.NumberFormat(${o(locale)}).formatToParts({valueOf(){return 12.5}})))`,
  );
}

// Deduplicação contra o que os goldens existentes já medem: chave normalizada (sem espaços) de
// `new Intl.NumberFormat(locale,opções).método(argumentos)`.
const keyRe = /new Intl\.NumberFormat\(("[^"]*")(?:,(\{[^}]*\}))?\)\.(formatToParts|formatRangeToParts|formatRange)\(([^()]*)\)/g;
const existing = new Set();
const dir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(dir)) {
  if (!file.endsWith(".tsv") || file === "number_matrix_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(dir, file), "utf8").split("\n")) {
    if (!line.includes("NumberFormat")) continue;
    let source = line.split("\t")[0];
    try { source = JSON.parse(source); } catch (e) { continue; }
    for (const m of source.replace(/\s+/g, "").matchAll(keyRe)) existing.add(m[0]);
  }
}

const seen = new Set();
const unique = [];
let dup = 0;
for (const expr of exprs) {
  if (seen.has(expr)) continue;
  seen.add(expr);
  const keys = [...expr.replace(/\s+/g, "").matchAll(keyRe)].map((m) => m[0]);
  if (keys.length && keys.every((k) => existing.has(k))) { dup++; continue; }
  unique.push(expr);
}

function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => resolve({ code, out, err }));
    child.stdin.end(source);
  });
}

async function main() {
  const sources = unique.map((expr) => '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`);
  const results = new Array(sources.length);
  let next = 0;
  const worker = async () => {
    for (;;) {
      const i = next++;
      if (i >= sources.length) return;
      results[i] = await runChild(sources[i]);
    }
  };
  await Promise.all(Array.from({ length: 12 }, worker));
  let kept = 0;
  let dropped = 0;
  const lines = [];
  for (let i = 0; i < sources.length; i++) {
    const r = results[i];
    if (r.code !== 0) { dropped++; process.stderr.write("filho falhou: " + unique[i].slice(0, 160) + " " + r.err.slice(0, 200) + "\n"); continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out)) { dropped++; process.stderr.write("caminho ou marca: " + unique[i].slice(0, 160) + "\n"); continue; }
    kept++;
    lines.push(JSON.stringify(sources[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("number_matrix", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
