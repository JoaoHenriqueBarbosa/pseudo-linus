// Gera tests/golden/number_range_bun.tsv: Intl.NumberFormat#formatRange e #formatRangeToParts em grade (15 locales, styles
// decimal, currency, percent e unit, notations standard, compact, scientific e engineering, pares de números iguais, próximos,
// invertidos, NaN, Infinity, BigInt e strings decimais longas), formatToParts com signDisplay, useGrouping, roundingMode,
// roundingIncrement e trailingZeroDisplay, e os RangeError/TypeError exatos, medidos no bun.
// Programas cujo `new Intl.NumberFormat(...).método(...)` já aparece em outro golden são descartados (knownPrograms).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Cada programa roda num bun filho novo, sem
// APIs de host, com timeout de 8 s e no máximo 6 filhos em paralelo.
// O travessão (U+2013) e o travessão longo (U+2014) saem do bun como escape JSON no tsv.
// Uso: bun scripts/gen-number-range-golden.js > tests/golden/number_range_bun.tsv
const { emitFactoredLines, knownPrograms, sampleByHash } = require("./golden-prelude.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function P(a){return a.map(function(x){return x.type+":"+x.value+(x.source?":"+x.source:"")}).join("|")}\n' +
  'function T(f){try{return f()}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const LOCALES = ["en-US", "en-IN", "pt-PT", "es-MX", "fr-CA", "de-CH", "it-IT", "ja-JP", "ko-KR", "zh-TW", "ru-RU", "hi-IN", "ar-EG", "pl-PL", "nl-NL"];

const STYLES = [
  { style: "decimal" },
  { style: "currency", currency: "EUR" },
  { style: "percent" },
  { style: "unit", unit: "kilometer-per-hour", unitDisplay: "long" },
];
const NOTATIONS = ["standard", "compact", "scientific", "engineering"];

const PAIRS = [
  ["5", "5"], ["1.5", "1.5001"], ["3", "1"], ["NaN", "1"], ["1", "Infinity"], ["Infinity", "Infinity"],
  ["5n", "99999999999999999999n"], ["'1234567890.123456789'", "'1234567890.123456790'"],
  ["-5", "5"], ["999", "1001"], ["0", "-0"], ["1e21", "2e21"],
];

const o = (x) => JSON.stringify(x);
const exprs = [];

// Grade de formatRange e formatRangeToParts. A metade das combinações ganha também o formatRangeToParts, escolhida por hash
// do texto do programa (`sampleByHash`), não pela soma dos índices.
const gridPartsCandidates = [];
for (const locale of LOCALES) {
  for (const style of STYLES) {
    for (const notation of NOTATIONS) {
      const opts = { ...style, notation };
      for (const [a, b] of PAIRS) {
        exprs.push(`T(()=>new Intl.NumberFormat(${o(locale)},${o(opts)}).formatRange(${a},${b}))`);
        gridPartsCandidates.push(`T(()=>P(new Intl.NumberFormat(${o(locale)},${o(opts)}).formatRangeToParts(${a},${b})))`);
      }
    }
  }
}
exprs.push(...sampleByHash(gridPartsCandidates, Math.ceil(gridPartsCandidates.length / 2)));

// Opções de arredondamento e sinal em formatRange.
const ROUND_OPTS = [
  { signDisplay: "always" }, { signDisplay: "exceptZero" }, { signDisplay: "negative" }, { signDisplay: "never" },
  { useGrouping: false }, { useGrouping: "min2" }, { useGrouping: "always" },
  { roundingMode: "floor", maximumFractionDigits: 0 }, { roundingMode: "ceil", maximumFractionDigits: 0 },
  { roundingMode: "halfEven", maximumFractionDigits: 1 }, { roundingMode: "trunc", maximumFractionDigits: 1 },
  { roundingIncrement: 5, maximumFractionDigits: 2, minimumFractionDigits: 2 }, { roundingIncrement: 50, maximumFractionDigits: 0 },
  { trailingZeroDisplay: "stripIfInteger", minimumFractionDigits: 2 },
  { minimumSignificantDigits: 3, maximumSignificantDigits: 3 },
];
const ROUND_PAIRS = [["1", "2"], ["0.995", "1.004"], ["-0.4", "0.4"], ["12.5", "12.51"], ["100", "100.001"], ["-3", "-1"]];
// Metade das combinações (locale x opções x par) entra, escolhida por hash do programa; cada uma leva as duas formas.
const roundCandidates = [];
for (const locale of LOCALES) {
  for (const opts of ROUND_OPTS) {
    for (const [a, b] of ROUND_PAIRS) {
      roundCandidates.push([
        `T(()=>new Intl.NumberFormat(${o(locale)},${o(opts)}).formatRange(${a},${b}))`,
        `T(()=>P(new Intl.NumberFormat(${o(locale)},${o(opts)}).formatRangeToParts(${a},${b})))`,
      ]);
    }
  }
}
for (const pair of sampleByHash(roundCandidates, Math.ceil(roundCandidates.length / 2), (pair) => pair[0])) exprs.push(...pair);

// formatToParts com signDisplay, useGrouping, roundingMode, roundingIncrement e trailingZeroDisplay.
const PART_OPTS = [];
for (const signDisplay of ["auto", "always", "exceptZero", "negative", "never"]) PART_OPTS.push({ signDisplay }, { signDisplay, style: "percent" }, { signDisplay, style: "currency", currency: "USD", currencySign: "accounting" });
for (const useGrouping of [true, false, "min2", "always", "auto"]) PART_OPTS.push({ useGrouping }, { useGrouping, notation: "compact" });
for (const roundingMode of ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"]) PART_OPTS.push({ roundingMode, maximumFractionDigits: 0 }, { roundingMode, maximumFractionDigits: 1 });
for (const roundingIncrement of [2, 5, 10, 25, 100, 500]) PART_OPTS.push({ roundingIncrement, minimumFractionDigits: 2, maximumFractionDigits: 2 }, { roundingIncrement, maximumFractionDigits: 0 });
for (const trailingZeroDisplay of ["auto", "stripIfInteger"]) PART_OPTS.push({ trailingZeroDisplay, minimumFractionDigits: 2 }, { trailingZeroDisplay, style: "currency", currency: "JPY" });
const PART_VALUES = ["0", "-0", "NaN", "-Infinity", "-1234.565", "0.045", "7n", "'-98765432109876543210.5'", "2.5", "-2.5", "1e21"];
// Dois valores por (locale, opções), escolhidos por hash do programa entre todos os valores.
const partCandidates = [];
for (const locale of LOCALES) {
  for (const opts of PART_OPTS) {
    for (const v of PART_VALUES) partCandidates.push(`T(()=>P(new Intl.NumberFormat(${o(locale)},${o(opts)}).formatToParts(${v})))`);
  }
}
exprs.push(...sampleByHash(partCandidates, LOCALES.length * PART_OPTS.length * 2));

// Erros exatos.
const BAD_CTORS = [
  "{style:'currency'}", "{style:'currency',currency:'US'}", "{style:'unit'}", "{style:'unit',unit:'bogus'}", "{style:'bogus'}",
  "{roundingIncrement:3}", "{roundingIncrement:5,maximumSignificantDigits:3}", "{maximumFractionDigits:101}", "{minimumFractionDigits:5,maximumFractionDigits:2}",
  "{signDisplay:'bogus'}", "{useGrouping:'bogus'}", "{roundingMode:'bogus'}", "{notation:'bogus'}", "{compactDisplay:'bogus'}",
  "{minimumIntegerDigits:0}", "{roundingPriority:'bogus'}", "{trailingZeroDisplay:'bogus'}", "{unit:'meter-per-bogus',style:'unit'}",
];
const CALLS = ["formatRange(NaN,1)", "formatRange(1,NaN)", "formatRange(NaN,NaN)", "formatRange(undefined,1)", "formatRange(1,undefined)", "formatRange()",
  "formatRange('x',1)", "formatRangeToParts(NaN,1)", "formatRangeToParts(1,undefined)", "formatRange(Symbol(),1)", "formatRange(1,Symbol())",
  "formatRange(1n,'2')", "formatRange(2,1)", "formatRange('abc',2)", "formatRangeToParts('1e1000','-1e1000')"];
for (const locale of LOCALES) {
  for (const call of CALLS) exprs.push(`T(()=>new Intl.NumberFormat(${o(locale)}).${call})`);
}
for (const locale of LOCALES.slice(0, 5)) {
  BAD_CTORS.forEach((opts) => exprs.push(`T(()=>new Intl.NumberFormat(${o(locale)},${opts}).formatRange(1,2))`));
}
for (const bad of ["'x-'", "'en_US'", "'a'", "''", "'en-US-u-nu-bogus'", "['en-US','1']", "5", "null"]) {
  exprs.push(`T(()=>new Intl.NumberFormat(${bad}).formatRange(1,2))`);
  exprs.push(`T(()=>Intl.NumberFormat.prototype.formatRange.call({},1,2))`);
}

// Deduplicação contra os programas dos goldens vizinhos: chave `new Intl.NumberFormat(locale,opções).método(args)`.
const keyRe = /new Intl\.NumberFormat\(("[^"]*")(?:,(\{[^}]*\}))?\)\.(formatToParts|formatRangeToParts|formatRange)\(([^()]*)\)/g;
const existing = new Set();
for (const source of knownPrograms("number_range_bun.tsv", (name) => name !== "number_range_bun.tsv")) {
  if (!source.includes("NumberFormat")) continue;
  for (const m of source.replace(/\s+/g, "").matchAll(keyRe)) existing.add(m[0]);
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
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 8000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => { clearTimeout(timer); resolve({ code: timedOut ? -1 : code, out, err }); });
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
  await Promise.all(Array.from({ length: 6 }, worker));
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
  // JSON.stringify deixa U+2013 e U+2014 literais; no tsv eles saem como escape JSON.
  const dash = String.fromCharCode(0x2013), longDash = String.fromCharCode(0x2014);
  const text = emitFactoredLines("number_range", lines).split(dash).join("\\u2013").split(longDash).join("\\u2014");
  process.stdout.write(text);
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
