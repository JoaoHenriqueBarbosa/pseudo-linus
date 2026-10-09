// Gera tests/golden/string_coerce_bun.tsv: grade de coerção de argumentos dos métodos de String.prototype, medida no bun.
// Métodos: at, charAt, charCodeAt, codePointAt, slice, substring, substr, padStart, padEnd, repeat, indexOf,
// lastIndexOf, includes, startsWith, endsWith, normalize, localeCompare (sem locale), isWellFormed, toWellFormed.
// Receptores: strings com surrogates soltos, vazias, longas, null/undefined (TypeError exato), números, objetos com
// toString. Argumentos: -0, NaN, ±Infinity, ±2**53, 1.5, '3', objetos com valueOf que registram a ordem ou lançam,
// Symbol (TypeError) e BigInt. Cada programa roda num bun filho novo, sem APIs de host, e grava o resultado em
// `globalThis.R`. Programas já presentes nos goldens de string vizinhos são descartados (knownPrograms).
// Uso: bun scripts/gen-string-coerce-golden.js > tests/golden/string_coerce_bun.tsv
const fs = require("fs");
const { emitFactored, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var L=[];\n' +
  'function S(v){try{if(typeof v==="string")return JSON.stringify(v);if(Object.is(v,-0))return "-0";if(typeof v==="bigint")return v+"n";' +
  'if(typeof v==="symbol")return v.toString();if(typeof v==="undefined")return "undefined";if(typeof v==="function")return "fn";' +
  'if(Array.isArray(v))return "["+v.map(S).join(",")+"]";return String(v)}catch(e){return "?"}}\n' +
  'function T(f){var r;try{r=S(f())}catch(e){r="throw "+(e&&e.name)+": "+(e&&e.message)}return L.length?r+" | log="+L.map(S).join(","):r}\n';

const logVo = (tag, value) => `{valueOf(){L.push('${tag}.vo');return ${value}},toString(){L.push('${tag}.ts');return String(${value})}}`;
const logTs = (tag, value) => `{toString(){L.push('${tag}.ts');return ${value}}}`;
const logPrim = (tag, value) => `{[Symbol.toPrimitive](h){L.push('${tag}.tp '+h);return ${value}}}`;

const receivers = [
  "'abcdef'", "''", "'a'", "'x'.repeat(300)+'END'", "'\\ud800'", "'a\\udc00b'", "'\\ud83d\\ude00z'", "'z\\ude00\\ud83d'", "'\\ud83d'+'\\ud83d\\ude00'",
  "undefined", "null", "12345", "-0", "NaN", "true", "[1,2,3]", logTs("r", "'objstr'"), logPrim("r", "'prim'"), "new String('boxed')",
  "{toString(){throw new RangeError('recv')}}", "Symbol('s')", "10n",
];
const smallReceivers = ["'abcdef'", "'\\ud83d\\ude00a\\ud800'", "undefined", logTs("r", "'abcdef'"), "12345", "''"];

const args = [
  "undefined", "null", "true", "NaN", "-0", "0", "1", "2", "3", "-1", "-2", "7", "100", "1.5", "-1.5", "0.9", "-0.9", "Infinity", "-Infinity",
  "2**31", "2**32", "2**53", "-(2**53)", "2**53+2", "1e21", "'3'", "' 2 '", "'0x2'", "'abc'", "''", "[]", "[2]", "({})",
  "1n", "Symbol()", "new Number(2)", "new String('1')",
  logVo("a", "2"), logVo("a", "-1"), logVo("a", "'3'"), logVo("a", "NaN"), logVo("a", "-0"), logVo("a", "Infinity"), logVo("a", "{}"),
  logPrim("a", "1"), logPrim("a", "'2'"), logPrim("a", "{}"), logPrim("a", "Symbol()"),
  "{valueOf(){throw new EvalError('arg')}}", "{valueOf(){L.push('a.vo');return 1n}}", "{[Symbol.toPrimitive]:1}",
];
const smallArgs = ["undefined", "NaN", "-0", "0", "1", "3", "-1", "Infinity", "-Infinity", "2**53", "1.5", "'2'", "null", "1n", "Symbol()",
  logVo("b", "2"), logVo("b", "-1"), "{valueOf(){throw new EvalError('b')}}"];
const needles = ["undefined", "null", "NaN", "-0", "1.5", "''", "'c'", "'cd'", "'abc'", "'\\ud83d'", "'\\ude00'", "'\\ud800'", "'END'", "[]", "['c']",
  "Symbol()", "1n", logTs("n", "'cd'"), logVo("n", "'e'"), logPrim("n", "'f'"), "{toString(){throw new RangeError('needle')}}", "/c/", "123"];

const calls = [];
const add = (method, recv, a) => calls.push({ method, recv, a });

const indexMethods = ["at", "charAt", "charCodeAt", "codePointAt", "repeat"];
for (const m of indexMethods) for (const r of receivers) for (const a of args) add(m, r, a);
for (const m of ["slice", "substring", "substr"]) {
  for (const r of smallReceivers) for (const a of args) add(m, r, a);
  for (const r of ["'abcdef'", logTs("r", "'abcdef'")]) for (const a of args) for (const b of smallArgs) add(m, r, `${a},${b}`);
  for (const r of receivers) add(m, r, ""), add(m, r, "1,3");
}
for (const m of ["padStart", "padEnd"]) {
  for (const r of smallReceivers) for (const a of args) add(m, r, a);
  const fills = ["undefined", "''", "'x'", "'ab'", "null", "0", "Symbol()", "1n", logTs("f", "'zz'"), logPrim("f", "'q'"), "' '", "{toString(){throw new TypeError('fill')}}"];
  for (const a of ["undefined", "NaN", "-0", "3", "8", "2**31", "2**53", "Infinity", "'7'", logVo("a", "7"), "Symbol()"]) for (const f of fills) add(m, "'abc'", `${a},${f}`);
  for (const r of receivers) add(m, r, "6,'xy'");
}
for (const m of ["indexOf", "lastIndexOf", "includes", "startsWith", "endsWith"]) {
  for (const r of smallReceivers) for (const n of needles) add(m, r, n);
  for (const r of ["'abcabc'", logTs("r", "'abcabc'"), "undefined"]) for (const n of ["'b'", "''", "'abc'", logTs("n", "'c'"), "Symbol()"]) for (const p of args) add(m, r, `${n},${p}`);
  for (const r of receivers) add(m, r, "'a'"), add(m, r, "");
}
const forms = ["undefined", "'NFC'", "'NFD'", "'NFKC'", "'NFKD'", "'nfc'", "'NFX'", "''", "null", "1", "true", "Symbol()", "1n", "[]", "['NFD']", logTs("f", "'NFD'"), logPrim("f", "'NFKC'"), "{toString(){throw new TypeError('form')}}"];
for (const r of ["'\\u00e9'", "'e\\u0301'", "'\\ufb01'", "'\\ud800'", "''", "undefined", "null", logTs("r", "'\\u00e9'"), "12", "Symbol()", "'\\u1e9b\\u0323'"]) for (const f of forms) add("normalize", r, f);
for (const r of smallReceivers.concat(["'b'", "'B'", "'\\ud800'", "'a\\u0301'", "'\\u00e1'", "null", "Symbol()", "1n"])) for (const n of needles.concat(["'B'", "'\\u00e1'", "'a\\u0301'"])) add("localeCompare", r, n);
for (const r of ["'b'", "undefined", logTs("r", "'b'")]) for (const n of ["'a'", "'a',undefined", "'a',null", "'a',1", "'a',Symbol()", "'a',{}", "'a','en'", "'a',logVo('l','en')".replace("logVo('l','en')", "{toString(){L.push('l');return 'en'}}")]) add("localeCompare", r, n);
for (const m of ["isWellFormed", "toWellFormed"]) {
  for (const r of receivers) for (const a of ["", "1", "Symbol()", logVo("x", "1")]) add(m, r, a);
  for (const r of ["'\\ud800'", "'\\udc00'", "'\\ud800\\udc00'", "'\\udc00\\ud800'", "'a\\ud800b\\udc00c'", "'\\ud83d\\ude00\\ud83d'", "'x'.repeat(100)+'\\ud800'", "'\\ud800'.repeat(5)", "''"]) add(m, r, "");
}

const sources = [];
const seen = new Set(knownPrograms("string_coerce_bun.tsv", (name) => /^string/.test(name) || /str/i.test(name)));
for (const c of calls) {
  const body = `String.prototype[${JSON.stringify(c.method)}].call(${c.recv}${c.a === "" ? "" : "," + c.a})`;
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = T(()=>${body})`;
  if (seen.has(source)) continue;
  seen.add(source);
  sources.push(source);
}

const TIMEOUT_MS = 8000;
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), TIMEOUT_MS);
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", () => {});
    child.on("close", (code) => { clearTimeout(timer); resolve(code === 0 ? decodeResult(out) : null); });
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(sources.length);
  let next = 0;
  await Promise.all(Array.from({ length: 6 }, async () => {
    while (next < sources.length) {
      const i = next++;
      results[i] = await runChild(sources[i]);
    }
  }));
  const dash = new RegExp(String.fromCharCode(0x2014) + "|" + String.fromCharCode(0x2013));
  const rows = [];
  let dropped = 0;
  for (let i = 0; i < sources.length; i++) {
    const result = results[i];
    if (result === null || dash.test(result) || /\/home\/|\/tmp\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(sources[i].slice(-120)) + "\n");
      continue;
    }
    rows.push({ source: sources[i], result });
  }
  process.stdout.write(emitFactored("string_coerce", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}\n`);
})();
