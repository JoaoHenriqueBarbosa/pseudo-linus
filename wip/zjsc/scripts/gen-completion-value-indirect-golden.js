// Gera tests/golden/completion_value_indirect_bun.tsv: valor de completude de eval indireto `(0, eval)(fonte)` para
// combinações de 2 a 4 statements (if/else, laços com break/continue rotulados, switch com fallthrough,
// try/catch/finally com e sem break no finally, blocos vazios, declarações, with, labels aninhados), medido no bun.
// Cada programa roda num bun filho novo (sem vazamento de `var` entre linhas), sem APIs de host, grava o texto em
// `globalThis.R` (typeof e valor; erro vira `throw Nome`), com timeout de 8 s e no máximo 6 filhos ao mesmo tempo.
// O prelúdio comum sai em tests/golden/completion_value_indirect.preludes.json (scripts/golden-prelude.js).
// Programas já presentes em outro golden (knownPrograms) são descartados.
// Uso: bun scripts/gen-completion-value-indirect-golden.js > tests/golden/completion_value_indirect_bun.tsv
const fs = require("fs");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v){if(v===undefined)return "undefined";if(v===null)return "null";if(Object.is(v,-0))return "-0";var t=typeof v;' +
  'if(t==="string")return "string:"+v;if(t==="number"||t==="boolean")return t+":"+String(v);return t}\n' +
  'function C(s){try{return S((0,eval)(s))}catch(e){return "throw "+(e&&e.name)}}\n';

// Tabela de statements. Cada um é um statement completo; os que usam rótulo ou break/continue trazem o contexto.
const STATEMENTS = [
  "1", "2;", ";", "{}", "{ 3 }", "{ 4; {} }", "var a", "var b = 5", "let c = 6", "const d = 7",
  "function f() {}", "class K {}", "'s'", "null", "debugger",
  "if (1) 8", "if (0) 9", "if (1) 10; else 11", "if (0) 12; else 13", "if (1) {} else 14", "if (0) 15; else {}",
  "if (1) { 16; if (0) 17 }", "if (0) ; else ;",
  "do { 18; break } while (0)", "do { 19; continue } while (0)", "do { break } while (0)", "do { continue } while (0)",
  "while (0) 20", "for (;false;);", "for (var i = 0; i < 2; i++) { i }", "for (var i = 0; i < 2; i++) { 21; if (i) break }",
  "for (var i = 0; i < 3; i++) { if (i == 1) continue; i }", "for (var k in {x: 1, y: 2}) k", "for (var o of [1, 2]) { o; continue }",
  "L: for (;;) { 22; break L }", "L: for (var i = 0; i < 2; i++) { 23; continue L }", "L: while (1) { break L }",
  "L: { 24; break L }", "L: { break L }", "L: 25", "L: ;", "M: { 26 }",
  "L: { M: { 27; break L } 28 }", "L: { M: { 29; break M } 30 }", "L: M: { 31; break L }",
  "L: for (;;) { M: for (;;) { 32; break L } }", "L: for (var i = 0; i < 2; i++) { M: for (var j = 0; j < 2; j++) { i + j; continue L } }",
  "switch (1) { case 1: 33 }", "switch (1) { case 1: 34; case 2: 35 }", "switch (1) { case 1: 36; break; case 2: 37 }",
  "switch (0) { case 1: 38 }", "switch (9) { default: 39 }", "switch (1) { default: 40; case 1: }", "switch (2) { case 1: 41; default: 42; case 3: 43 }",
  "switch (1) { case 1: 44; if (1) break; 45 }", "switch (1) { case 1: { break } }",
  "try { 46 } finally { 47 }", "try { 48 } catch (e) { 49 }", "try { throw 0 } catch (e) { 50 }", "try { throw 0 } catch (e) { }",
  "try { 51 } finally { }", "try { } finally { 52 }", "try { throw 0 } catch (e) { 53 } finally { 54 }",
  "do { try { 55; break } finally { 56 } } while (0)", "do { try { 57 } finally { break } } while (0)", "do { 58; try { 59 } finally { break } } while (0)",
  "do { try { 60 } finally { 61; continue } } while (0)", "L: try { 62; break L } finally { 63 }", "L: try { 64 } finally { break L }",
  "for (var i = 0; i < 2; i++) { try { i; continue } finally { 65 } }", "for (var i = 0; i < 2; i++) { try { i } finally { continue } }",
  "for (var i = 0; i < 2; i++) { try { throw i } catch (e) { e; break } finally { 66 } }",
  "with ({}) 67", "with ({}) {}", "with ({ a: 68 }) a", "with ({}) { 69; if (0) 70 }", "L: with ({}) { 71; break L }",
  "do { with ({}) { 72; break } } while (0)", "{ var v1 = 73 }", "{ function g() {} }", "{ let h = 74 }",
  "var x = 75; x", "x = 76", "void 0", "typeof 1",
];

// Gerador pseudoaleatório determinístico (xorshift32), para a amostra ser reproduzível.
let seed = 0x9e3779b9;
function rand(n) {
  seed ^= seed << 13; seed >>>= 0;
  seed ^= seed >>> 17;
  seed ^= seed << 5; seed >>>= 0;
  return seed % n;
}

const SEPARATORS = ["; ", "\n", " "];
const WRAPPERS = [(s) => s, (s) => s, (s) => `{ ${s} }`, (s) => `L: { ${s} }`, (s) => `do { ${s}; break } while (0)`, (s) => `with ({}) { ${s} }`,
  (s) => `if (1) { ${s} }`, (s) => `try { ${s} } finally { }`, (s) => `Z: { ${s} } 77`, (s) => `0; ${s}`];

function join(parts) {
  let text = parts[0];
  for (let i = 1; i < parts.length; i++) {
    const previous = parts[i - 1];
    // Statement terminado em bloco aceita espaço; os outros precisam de `;` ou quebra de linha.
    const separator = SEPARATORS[rand(SEPARATORS.length)];
    const needsTerminator = separator === " " && !/[};]$/.test(previous);
    text += (needsTerminator ? "; " : separator) + parts[i];
  }
  return text;
}

const sources = new Set();
// Todos os pares ordenados, sem embrulho e embrulhados.
for (const a of STATEMENTS) for (const b of STATEMENTS) {
  sources.add(join([a, b]));
  sources.add(WRAPPERS[2 + rand(WRAPPERS.length - 2)](join([a, b])));
}
// Triplas e quádruplas por amostragem.
for (let n = 0; n < 4500; n++) {
  const count = 3 + (n % 2);
  const parts = [];
  for (let i = 0; i < count; i++) parts.push(STATEMENTS[rand(STATEMENTS.length)]);
  sources.add(WRAPPERS[rand(WRAPPERS.length)](join(parts)));
}

const known = new Set(knownPrograms("completion_value_indirect_bun.tsv", (name) => name !== "completion_value_indirect_bun.tsv"));
const programs = [];
let duplicated = 0;
for (const source of sources) {
  const text = PRELUDE + `globalThis.R=C(${JSON.stringify(source)});`;
  if (known.has(text) || usesHostApi(text)) { duplicated++; continue; }
  programs.push(text);
}

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let done = false;
    const finish = (value) => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => { child.kill("SIGKILL"); finish(null); }, 8000);
    child.stdout.on("data", (d) => (out += d));
    child.on("error", () => finish(null));
    child.on("close", (code) => finish(code === 0 ? decodeResult(out) : null));
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  await Promise.all(Array.from({ length: 6 }, async () => {
    while (next < programs.length) {
      const i = next++;
      results[i] = await runChild(programs[i]);
    }
  }));
  const rows = [];
  let dropped = 0;
  programs.forEach((source, i) => {
    const result = results[i];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(source).slice(0, 200) + "\n");
      return;
    }
    rows.push({ source, result });
  });
  process.stdout.write(emitFactored("completion_value_indirect", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos ou de host ${duplicated}\n`);
})();
