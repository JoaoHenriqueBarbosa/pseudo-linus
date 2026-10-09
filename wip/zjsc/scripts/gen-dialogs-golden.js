// Gera tests/golden/dialogs_bun.tsv: `alert`, `confirm` e `prompt` do global medidos no bun 1.4.2 com stdin em EOF
// (descritor, `length`, `name`, chaves próprias, `new`, retorno `undefined`/`false`/`null`, conversão dos argumentos,
// ordem de chaves, atribuição e `delete`). O texto que o bun escreve no stdout não entra: o programa só lê `R`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Um processo `bun` por linha.
// Uso: bun scripts/gen-dialogs-golden.js > tests/golden/dialogs_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v === undefined) return 'undefined'; if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message };\n" +
  "var D = function (o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x && [typeof x.value, x.writable, x.enumerable, x.configurable, typeof x.get, typeof x.set] };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const stmts = (code) => programs.push(HELPER + `try { ${code} } catch (e) { R = E(e) }`);
const strict = (code) => programs.push(HELPER + `try { (function () { 'use strict'; ${code} })() } catch (e) { R = E(e) }`);
const names = "Object.getOwnPropertyNames(globalThis)";

for (const n of ["alert", "confirm", "prompt"]) {
  // Descritor, forma da função.
  expr(`D(globalThis, '${n}')`);
  expr(`typeof ${n}`);
  expr(`${n}.length`);
  expr(`${n}.name`);
  expr(`Object.getOwnPropertyNames(${n})`);
  expr(`String(${n})`);
  expr(`'prototype' in ${n}`);
  expr(`Object.getPrototypeOf(${n}) === Function.prototype`);
  expr(`D(${n}, 'name')`);
  expr(`D(${n}, 'length')`);
  expr(`self.${n} === ${n}`);
  expr(`Object.keys(globalThis).indexOf('${n}') >= 0`);

  // Construtor.
  expr(`new ${n}()`);
  expr(`Reflect.construct(${n}, [])`);

  // Retorno e conversão dos argumentos (stdin em EOF).
  expr(`${n}()`);
  expr(`${n}(undefined)`);
  expr(`${n}(null)`);
  expr(`${n}('x')`);
  expr(`${n}(1, 2)`);
  expr(`${n}('a', 'b', 'c')`);
  expr(`${n}({})`);
  expr(`${n}(1n)`);
  expr(`${n}(Symbol('s'))`);
  expr(`${n}(function () {})`);
  expr(`${n}.call(5, 'a')`);
  expr(`${n}.call(undefined, 'a')`);
  stmts(`var log = []; ${n}({ toString() { log.push('t'); return 'q' } }); R = S(log)`);
  stmts(`var log = []; ${n}('a', { toString() { log.push('t'); return 'q' } }); R = S(log)`);
  stmts(`var log = []; ${n}({ toString() { log.push('a'); return 'q' } }, { toString() { log.push('b'); return 'q' } }); R = S(log)`);
  expr(`${n}({ toString() { throw new RangeError('boom') } })`);
  expr(`${n}('a', { toString() { throw new RangeError('boom') } })`);
  expr(`${n}(undefined, { toString() { throw new RangeError('boom') } })`);
  expr(`${n}('a', Symbol('s'))`);
  expr(`${n}({ toString() { return Symbol('s') } })`);

  // Ordem de chaves.
  expr(`${names}.indexOf('${n}')`);

  // Atribuição, redefinição e delete.
  stmts(`${n} = 5; R = S([typeof ${n}, D(globalThis, '${n}')])`);
  strict(`${n} = 5; R = S([typeof ${n}, D(globalThis, '${n}')])`);
  stmts(`var i = ${names}.indexOf('${n}'); ${n} = 5; R = S(${names}.indexOf('${n}') - i)`);
  stmts(`R = S([delete globalThis.${n}, typeof ${n}, '${n}' in globalThis, D(globalThis, '${n}')])`);
  strict(`delete globalThis.${n}; ${n}(1)`);
  stmts(`var f = ${n}; delete globalThis.${n}; R = S([f('a'), f.name, f.length])`);
  stmts(`Object.defineProperty(globalThis, '${n}', { value: 7 }); R = S([${n}, D(globalThis, '${n}')])`);
  stmts(`${n}.extra = 1; R = S([${n}.extra, Object.keys(${n})])`);
  stmts(`${n}.name = 'x'; R = S(${n}.name)`);
  strict(`${n}.length = 9`);
}

// Posições relativas no global.
expr(`${names}.indexOf('alert') - ${names}.indexOf('addEventListener')`);
expr(`${names}.indexOf('atob') - ${names}.indexOf('alert')`);
expr(`${names}.indexOf('confirm') - ${names}.indexOf('clearTimeout')`);
expr(`${names}.indexOf('dispatchEvent') - ${names}.indexOf('confirm')`);
expr(`${names}.indexOf('prompt') - ${names}.indexOf('postMessage')`);
expr(`${names}.indexOf('queueMicrotask') - ${names}.indexOf('prompt')`);

// Modo `io` (`bun scripts/gen-dialogs-golden.js io > tests/golden/dialogs_io_bun.tsv`): o convite que `alert`,
// `confirm` e `prompt` escrevem no stdout e a leitura da linha do stdin. Colunas: stdin (JSON), fonte (JSON), stdout
// capturado antes do resultado (JSON) e o valor de `R` (JSON). O stdin é fornecido por inteiro ao processo `bun`.
if (process.argv[2] === "io") {
  const cases = [];
  const io = (stdin, code) => cases.push([stdin, HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`]);
  for (const n of ["alert", "confirm", "prompt"]) {
    for (const stdin of ["", "\n", "y\n", "Y\n", "yes\n", "n\n", " y\n", "y", "abc\n", "abc", "a\r\n", "a\r\r\n", "a\r", "\r\n", "é\n", "a\nb\n"]) {
      io(stdin, `${n}()`);
      io(stdin, `${n}('q')`);
    }
    io("", `${n}('')`);
    io("", `${n}(undefined)`);
    io("", `${n}(null)`);
    io("", `${n}(1, 2)`);
    io("", `${n}('a\\nb')`);
    io("", `${n}({ toString() { return 'obj' } })`);
    io("", `${n}({ toString() { throw new RangeError('boom') } })`);
    io("", `${n}(Symbol('s'))`);
    io("y\n", `${n}.call(5, 'a')`);
    io("y\n", `[${n}('a'), ${n}('b')]`);
    io("y\nn\n", `[${n}('a'), ${n}('b')]`);
    io("y\n\nz\n", `[${n}('a'), ${n}('b'), ${n}('c')]`);
  }
  // Surrogate solto na mensagem: o bun escreve U+FFFD (EF BF BD) por unidade solta; par válido vira 4 bytes UTF-8.
  for (const n of ["alert", "confirm", "prompt"]) {
    for (const message of ["a\\ud800b", "a\\udc00b", "\\ud83d\\ude00", "\\ud83d", "\\ude00\\ud83d", "\\ud83d\\ud83d\\ude00", "\\u0000x\\u00e9", "\\ud800"]) io("", `${n}('${message}')`);
  }
  io("", "prompt('a\\ud800', '\\udc00x')");
  io("", "prompt('x', '\\ud83d')");
  io("\n", "prompt('x', '\\ud800y')");
  io("\n", "prompt('x', '\\ud800')");
  // O padrão do prompt: com segundo argumento o convite leva `[padrão]`, linha vazia devolve o padrão.
  for (const stdin of ["", "\n", "\r\n", "abc\n", "abc"]) {
    for (const def of ["'d'", "''", "undefined", "null", "5", "{ toString() { return 'obj' } }"]) io(stdin, `prompt('x', ${def})`);
    io(stdin, "prompt()");
    io(stdin, "prompt(undefined, 'd')");
    io(stdin, "prompt('x', 'd', 'e')");
  }
  io("", "prompt('x', { toString() { throw new RangeError('boom') } })");
  io("", "prompt('x', Symbol('s'))");
  io("", "prompt({ toString() { throw new RangeError('first') } }, { toString() { throw new RangeError('second') } })");
  io("", "(function () { var log = []; prompt({ toString() { log.push('a'); return 'q' } }, { toString() { log.push('b'); return 'd' } }); return log })()");
  for (const [stdin, source] of cases) {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "dl-"));
    const file = path.join(dir, "case.js");
    fs.writeFileSync(
      file,
      `(0, eval)("var R");\n(0, eval)(${JSON.stringify(sourceAscii)});\nprocess.stdout.write("\\u0000R" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
    );
    // Sem `encoding`: o stdout fica em bytes, e a terceira coluna é o hex deles (estável mesmo com U+FFFD ou lixo).
    const run = spawnSync(process.execPath, [file], { timeout: 15000, input: stdin });
    fs.rmSync(dir, { recursive: true, force: true });
    if (run.status !== 0) throw new Error("bun falhou em: " + sourceAscii + "\n" + run.stderr);
    const at = run.stdout.lastIndexOf(Buffer.from("\u0000R"));
    emitRow([JSON.stringify(stdin), JSON.stringify(sourceAscii), JSON.stringify(run.stdout.subarray(0, at).toString("hex")), run.stdout.subarray(at + 2).toString("utf8")].join("\t"));
  }
  process.exit(0);
}

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "dl-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `(0, eval)("var R");\n(0, eval)(${JSON.stringify(sourceAscii)});\nprocess.stdout.write("\\n" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
  );
  const run = spawnSync(process.execPath, [file], { encoding: "utf8", timeout: 15000, input: "" });
  fs.rmSync(dir, { recursive: true, force: true });
  if (run.status !== 0) throw new Error("bun falhou em: " + sourceAscii + "\n" + run.stderr);
  // O convite escrito pelo bun no stdout vem antes: a última linha é o resultado.
  emitRow(JSON.stringify(sourceAscii) + "\t" + run.stdout.slice(run.stdout.lastIndexOf("\n") + 1));
}
