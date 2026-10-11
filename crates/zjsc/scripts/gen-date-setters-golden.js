// Gera tests/golden/date_setters_bun.tsv: setters de Date medidos no bun 1.4.2, no fuso America/Sao_Paulo (TZ no env dos
// filhos). Cobre setFullYear, setMonth, setDate, setHours, setMinutes, setSeconds, setMilliseconds, as versões UTC,
// setYear e setTime, com argumentos extras, a menos de argumentos, NaN, Infinity, strings, objetos com valueOf que
// registra a ordem das coerções, valueOf que lança, Symbol e BigInt. As datas são válidas perto de transições
// históricas de horário de verão de São Paulo (1931, 1985, 2008, 2018, 2019), inválidas, e os limites de +-8.64e15.
// Saída de cada programa: log das coerções, retorno do setter (ou a exceção), getTime, toISOString, toString e
// getTimezoneOffset da data depois da chamada.
// Programas já presentes nos goldens date_* são descartados. Cada programa roda num bun filho novo (6 em paralelo,
// timeout de 8 s). Colunas: fonte (JSON) e valor da global `R` (JSON).
// Uso: bun scripts/gen-date-setters-golden.js > tests/golden/date_setters_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
const { spawn } = require("child_process");

const ZONE = "America/Sao_Paulo";

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);return Object.is(v,-0)?"-0":String(v)}\n' +
  'function T(f){try{return f()}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const RUN = (date, call) =>
  `T(()=>{var l=[];function V(n,v){return{valueOf(){l.push("v"+n);return v}}}` +
  `function X(n){return{valueOf(){l.push("t"+n);throw new RangeError("x"+n)}}}` +
  `var d=${date};var r;try{r=S(${call})}catch(e){r="!"+e.name+": "+e.message}` +
  `var s;try{s=d.toISOString()}catch(e){s="!"+e.name}` +
  `return l.join()+"|"+r+"|"+S(d.getTime())+"|"+s+"|"+d.toString()+"|"+d.getTimezoneOffset()})`;

const dates = [
  "new Date(2018,10,3,23,59,59,999)", "new Date(2018,10,4,1,0)", "new Date(2019,1,16,23,30)", "new Date(2019,1,17,12)",
  "new Date(1985,10,2,12)", "new Date(1986,2,15,23,59)", "new Date(1931,9,3,11,0)", "new Date(1932,3,1,0,0)",
  "new Date(2008,9,18,12)", "new Date(2008,9,19,1,0)", "new Date(2016,9,15,12)", "new Date(1969,11,31,23,59,59)",
  "new Date(0)", "new Date(1900,0,1)", "new Date(2020,1,29,0,0,0,0)", "new Date(8.64e15)", "new Date(-8.64e15)",
  "new Date(8.64e15-1)", "new Date(-8.64e15+1)", "new Date(8.64e15-86400000*40)", "new Date(-8.64e15+86400000*40)",
  "new Date(NaN)", "new Date(2018,10,4,0,0)", "new Date(2019,1,17,0,0)",
];

// Valores de argumento; `@` vira o índice da posição, para o log mostrar a ordem das coerções.
const values = [
  "0", "1", "-1", "2", "12", "31", "32", "59", "60", "100", "1e3", "1e9", "NaN", "Infinity", "-Infinity", "'5'", "''", "null",
  "undefined", "true", "1.9", "-1.9", "0.5", "-0.5", "-0", "275760", "-271821", "275761", "1970", "2018", "2019", "1985",
  "1e15", "8.64e15", "-8.64e15", "9007199254740993", "[]", "[7]", "'abc'", "' 12 '", "0x10", "new Number(4)",
  "V(@,3)", "V(@,NaN)", "V(@,Infinity)", "V(@,-1)", "V(@,'11')", "X(@)", "X(@)", "Symbol()", "1n", "({})",
  "{valueOf:null,toString(){l.push('s@');return '9'}}", "V(@,1e20)", "V(@,23)", "V(@,59)", "V(@,999)",
];

// [nome, aridade]
const setters = [
  ["setFullYear", 3], ["setMonth", 2], ["setDate", 1], ["setHours", 4], ["setMinutes", 3], ["setSeconds", 2],
  ["setMilliseconds", 1], ["setUTCFullYear", 3], ["setUTCMonth", 2], ["setUTCDate", 1], ["setUTCHours", 4],
  ["setUTCMinutes", 3], ["setUTCSeconds", 2], ["setUTCMilliseconds", 1], ["setYear", 1], ["setTime", 1],
];

let seed = 20261008;
const rnd = (n) => {
  seed = (Math.imul(seed, 1103515245) + 12345) >>> 0;
  return (seed >>> 8) % n;
};

const exprs = [];
const add = (e) => exprs.push(e);
const PER_PAIR = 11;
for (const date of dates) {
  for (const [name, arity] of setters) {
    // Contagens de argumentos: 0, a aridade, uma a mais, duas a mais, e o resto sorteado.
    const counts = [0, 1, arity, arity + 1, arity + 2];
    while (counts.length < PER_PAIR) counts.push(rnd(arity + 3));
    for (const count of counts) {
      const args = Array.from({ length: count }, (_, i) => values[rnd(values.length)].replace(/@/g, String(i)));
      add(RUN(date, `d.${name}(${args.join(",")})`));
    }
  }
}
// Duas chamadas em sequência na mesma data (o segundo setter parte do estado do primeiro, inclusive de NaN).
for (const date of dates.slice(0, 16)) {
  for (let i = 0; i < 12; i++) {
    const [a, arityA] = setters[rnd(setters.length)];
    const [b, arityB] = setters[rnd(setters.length)];
    const argsA = Array.from({ length: arityA }, (_, k) => values[rnd(values.length)].replace(/@/g, String(k)));
    const argsB = Array.from({ length: arityB }, (_, k) => values[rnd(values.length)].replace(/@/g, String(k + 5)));
    add(RUN(date, `[d.${a}(${argsA.join(",")}),d.${b}(${argsB.join(",")})].map(S).join()`));
  }
}

// ---- Execução.
const baseText = knownPrograms("date_setters_bun.tsv", (name) => /^date_/.test(name)).join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let dup = 0;
let dropped = 0;
const jobs = [];
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  jobs.push('"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`);
}
const DASH = new RegExp("[" + String.fromCharCode(0x2013) + String.fromCharCode(0x2014) + "]");
const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, TZ: ZONE } });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", (d) => { err += d; });
    child.on("close", (code) => {
      clearTimeout(timer);
      const result = decodeResult(out);
      resolve(code === 0 && result !== null ? { out: result } : { err: err || "filho falhou" });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}
async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i]);
    }
  });
  await Promise.all(workers);
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.err !== undefined || r.out === "<undefined>") { dropped++; continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || DASH.test(r.out) || DASH.test(jobs[i]) || usesHostApi(jobs[i].slice(PRELUDE.length))) { dropped++; continue; }
    lines.push(JSON.stringify(jobs[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("date_setters", lines));
  process.stderr.write(`mantidos ${lines.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
