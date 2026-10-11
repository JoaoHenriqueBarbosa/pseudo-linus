// Gera tests/golden/brand_bun.tsv: verificação de this/brand dos métodos e getters de protótipo dos builtins, medida no
// bun 1.4.2. Os protótipos e as chaves são enumerados aqui com Reflect.ownKeys/getOwnPropertyDescriptor; cada linha do
// golden é um programa para UM método e UM this. O resultado (variável global `R`) é `typeof valor` ou `Nome|mensagem`.
// Colunas: a fonte do programa (JSON) e o valor de `R` (JSON), igual a gen-function-error-golden.js.
// Uso: bun scripts/gen-brand-check-golden.js > tests/golden/brand_bun.tsv
const fs = require("fs");
const { emitFactoredLines, prepareProgram } = require("./golden-prelude.js");
const { sampleByHash } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const TARGET = 1500;

// [nome para exibir, expressão do protótipo, expressão de instância legítima de subclasse (ou null), expressão de objeto de outra classe, desembrulha promessa]
const generatorProto = "Object.getPrototypeOf(function* () {}).prototype";
const asyncGeneratorProto = "Object.getPrototypeOf(async function* () {}).prototype";
const typedProto = "Object.getPrototypeOf(Uint8Array.prototype)";
const classes = [
  ["Map", "Map.prototype", "new (class X extends Map {})()"],
  ["Set", "Set.prototype", "new (class X extends Set {})()"],
  ["WeakMap", "WeakMap.prototype", "new (class X extends WeakMap {})()"],
  ["WeakSet", "WeakSet.prototype", "new (class X extends WeakSet {})()"],
  ["WeakRef", "WeakRef.prototype", "new (class X extends WeakRef {})({})"],
  ["FinalizationRegistry", "FinalizationRegistry.prototype", "new (class X extends FinalizationRegistry {})(() => {})"],
  ["Promise", "Promise.prototype", "new (class X extends Promise {})(() => {})"],
  ["Date", "Date.prototype", "new (class X extends Date {})(0)"],
  ["RegExp", "RegExp.prototype", "new (class X extends RegExp {})('a')"],
  ["ArrayBuffer", "ArrayBuffer.prototype", "new (class X extends ArrayBuffer {})(8)"],
  ["SharedArrayBuffer", "SharedArrayBuffer.prototype", "new (class X extends SharedArrayBuffer {})(8)"],
  ["DataView", "DataView.prototype", "new (class X extends DataView {})(new ArrayBuffer(8))"],
  ["TypedArray", typedProto, "new (class X extends Uint8Array {})(4)"],
  ["Uint8Array", "Uint8Array.prototype", "new (class X extends Uint8Array {})(4)"],
  ["Float64Array", "Float64Array.prototype", "new (class X extends Float64Array {})(4)"],
  ["Symbol", "Symbol.prototype", null],
  ["BigInt", "BigInt.prototype", null],
  ["Error", "Error.prototype", "new (class X extends Error {})('m')"],
  ["Function", "Function.prototype", "new (class X extends Function {})()"],
  ["Array", "Array.prototype", "new (class X extends Array {})(3)"],
  ["String", "String.prototype", "new (class X extends String {})('abc')"],
  ["Number", "Number.prototype", "new (class X extends Number {})(1)"],
  ["Boolean", "Boolean.prototype", "new (class X extends Boolean {})(true)"],
  ["Iterator", "Iterator.prototype", "new (class X extends Iterator {})()"],
  ["Generator", generatorProto, "(function* () {})()"],
  ["AsyncGenerator", asyncGeneratorProto, "(async function* () {})()"],
  ["Temporal.Duration", "Temporal.Duration.prototype", "new Temporal.Duration(1)"],
  ["Temporal.Instant", "Temporal.Instant.prototype", "new Temporal.Instant(0n)"],
  ["Temporal.PlainDate", "Temporal.PlainDate.prototype", "new Temporal.PlainDate(2020, 1, 1)"],
  ["Temporal.PlainTime", "Temporal.PlainTime.prototype", "new Temporal.PlainTime()"],
  ["Temporal.PlainDateTime", "Temporal.PlainDateTime.prototype", "new Temporal.PlainDateTime(2020, 1, 1)"],
  ["Temporal.PlainYearMonth", "Temporal.PlainYearMonth.prototype", "new Temporal.PlainYearMonth(2020, 1)"],
  ["Temporal.PlainMonthDay", "Temporal.PlainMonthDay.prototype", "new Temporal.PlainMonthDay(1, 1)"],
  ["Temporal.ZonedDateTime", "Temporal.ZonedDateTime.prototype", "new Temporal.ZonedDateTime(0n, 'UTC')"],
  ["Intl.Collator", "Intl.Collator.prototype", "new Intl.Collator()"],
  ["Intl.DateTimeFormat", "Intl.DateTimeFormat.prototype", "new Intl.DateTimeFormat()"],
  ["Intl.DisplayNames", "Intl.DisplayNames.prototype", "new Intl.DisplayNames('en', { type: 'region' })"],
  ["Intl.DurationFormat", "Intl.DurationFormat.prototype", "new Intl.DurationFormat()"],
  ["Intl.ListFormat", "Intl.ListFormat.prototype", "new Intl.ListFormat()"],
  ["Intl.Locale", "Intl.Locale.prototype", "new Intl.Locale('en')"],
  ["Intl.NumberFormat", "Intl.NumberFormat.prototype", "new Intl.NumberFormat()"],
  ["Intl.PluralRules", "Intl.PluralRules.prototype", "new Intl.PluralRules()"],
  ["Intl.RelativeTimeFormat", "Intl.RelativeTimeFormat.prototype", "new Intl.RelativeTimeFormat()"],
  ["Intl.Segmenter", "Intl.Segmenter.prototype", "new Intl.Segmenter()"],
];

const wellKnown = { "Symbol.iterator": 1, "Symbol.asyncIterator": 1, "Symbol.hasInstance": 1, "Symbol.toPrimitive": 1, "Symbol.toStringTag": 1, "Symbol.species": 1, "Symbol.match": 1, "Symbol.matchAll": 1, "Symbol.replace": 1, "Symbol.search": 1, "Symbol.split": 1, "Symbol.unscopables": 1, "Symbol.isConcatSpreadable": 1, "Symbol.dispose": 1, "Symbol.asyncDispose": 1 };

// ---- Enumeração no próprio bun: [classe, expressão do protótipo, chave (JSON ou expressão de símbolo), "get"|"value", asyncGen]
const enumerated = [];
for (const [name, protoExpr, subclassExpr] of classes) {
  let proto;
  try {
    proto = (0, eval)(protoExpr);
  } catch {
    continue;
  }
  if (!proto) continue;
  let subclassOk = false;
  if (subclassExpr) {
    try {
      (0, eval)(subclassExpr);
      subclassOk = true;
    } catch {}
  }
  for (const key of Reflect.ownKeys(proto)) {
    if (key === "constructor") continue;
    const d = Reflect.getOwnPropertyDescriptor(proto, key);
    let keyExpr;
    if (typeof key === "symbol") {
      const text = key.toString().slice(7, -1);
      if (!wellKnown[text]) continue;
      keyExpr = text;
    } else {
      keyExpr = JSON.stringify(key);
    }
    for (const kind of ["get", "set", "value"]) {
      if (typeof d[kind] !== "function") continue;
      enumerated.push({ name, protoExpr, subclassExpr: subclassOk ? subclassExpr : null, keyExpr, kind, async: name === "AsyncGenerator" });
    }
  }
}

const otherFor = name => (name === "Map" || name.startsWith("Intl.") || name.startsWith("Temporal.") ? "new Set()" : "new Map()");
const thisValues = entry => {
  const values = [
    ["undefined", "void 0"],
    ["null", "null"],
    ["one", "1"],
    ["str", "'x'"],
    ["obj", "{}"],
    ["arr", "[]"],
    ["other", otherFor(entry.name)],
    ["proxy", `new Proxy(${otherFor(entry.name)}, {})`],
  ];
  if (entry.subclassExpr) values.push(["sub", entry.subclassExpr]);
  return values;
};

let programs = [];
for (const entry of enumerated) {
  for (const [label, expr] of thisValues(entry)) {
    const keyArg = entry.keyExpr.startsWith("Symbol.") ? entry.keyExpr : entry.keyExpr;
    const body = [
      `var f = Reflect.getOwnPropertyDescriptor(${entry.protoExpr}, ${keyArg}).${entry.kind};`,
      `var fail = function (e) { R = e.name + '|' + e.message };`,
      entry.async
        ? `try { Promise.resolve(Reflect.apply(f, ${expr}, [])).then(function (v) { R = typeof v }, fail) } catch (e) { fail(e) }`
        : entry.kind === "set"
          ? `try { R = typeof Reflect.apply(f, ${expr}, [1]) } catch (e) { fail(e) }`
          : `try { R = typeof Reflect.apply(f, ${expr}, []) } catch (e) { fail(e) }`,
    ].join("\n");
    programs.push({ body, label, entry });
  }
}

// ---- Redução determinística até ~TARGET programas: mantém todo this "sub" e "undefined", afina o resto por hash (sampleByHash).
process.stderr.write(`enumerados ${programs.length} programas de ${enumerated.length} métodos\n`);
if (programs.length > TARGET * 1.15) {
  const must = programs.filter(p => p.label === "sub" || p.label === "undefined");
  const rest = programs.filter(p => p.label !== "sub" && p.label !== "undefined");
  const room = Math.max(0, TARGET - must.length);
  const picked = new Set(sampleByHash(rest, room, (p) => p.body));
  programs = programs.filter(p => p.label === "sub" || p.label === "undefined" || picked.has(p));
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "brand-golden-"));
const file = path.join(dir, "brand_case.js");
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('unhandledRejection', () => {})\nprocess.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const seen = new Set();
const lines = [];
let kept = 0;
let dropped = 0;
for (const { body } of programs) {
  if (seen.has(body)) continue;
  seen.add(body);
  const original = '"use strict";\n' + body.replace(/\bR = /g, "globalThis.R = ");
  // O bun transpila o arquivo antes do JSC (colunas e `evaluating '...'` citam o texto transpilado): grava-se o texto
  // canônico e o bun executa `executableSource(original)` (ver golden-prelude.js).
  const { source, executable, meta } = prepareProgram(original);
  fs.writeFileSync(file, executable);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 15000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1));
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  kept++;
  lines.push(JSON.stringify(source) + "	" + JSON.stringify(result) + (meta ? "	" + JSON.stringify(meta) : ""));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactoredLines("brand", lines));
fs.rmSync(dir, { recursive: true, force: true });
