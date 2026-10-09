// Base comum dos geradores de golden de microtarefas e promises (microtask, promise, promise_more, promise_grid).
// O bun transpila o programa quando ele roda como arquivo, então o programa é um arquivo inteiro: o harness (por padrão
// tests/golden/async_bun_harness.js: `log`, `L`, `tick`, `thenable`, `__final`) como começo, o corpo do caso dentro de
// um `try { } catch` que grava o erro síncrono em `__err`, e uma global de resultado (por padrão `R`), um getter que
// devolve `__final()`. O gerador grava `prepareProgram(original)` (ver golden-prelude.js), executa
// `executableSource(original)` com um preload que imprime o resultado no `exit` (as microtarefas já esvaziaram) e o teste
// Rust lê a global com `common::run_mapped_golden` (ou `evaluate_golden_program` com o nome dela).
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");
const { knownProgramSet, prepareProgram } = require("./golden-prelude.js");

const DEFAULT_HARNESS = fs.readFileSync(path.join(__dirname, "..", "tests", "golden", "async_bun_harness.js"), "utf8");
const BODY_START = "try {\n";
const BODY_END = "\n} catch (e) { globalThis.__err = ";

// O corpo de um programa montado por `originalProgram` (cru ou canônico); programa de outra forma volta inteiro. Serve
// para deduplicar contra goldens vizinhos, que guardam o programa completo.
function bodyKey(program) {
  const start = program.lastIndexOf(BODY_START);
  const end = program.lastIndexOf(BODY_END);
  return start >= 0 && end > start ? program.slice(start + BODY_START.length, end) : program;
}

// Roda `command` e devolve { code, out, err }; mata o filho depois de `timeoutMs`.
function spawnOnce(command, args, cwd, timeoutMs) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd });
    let out = "";
    let err = "";
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", (chunk) => (err += chunk));
    const timer = setTimeout(() => child.kill("SIGKILL"), timeoutMs);
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code, out, err });
    });
  });
}

// Harness dos goldens de ordem (microtask_order, queue_microtask): o log é a string global `R`, sem `__final`.
const ORDER_HARNESS = `globalThis.R = "";
globalThis.L = function (x) { R += x + ";"; };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.ok = function (v) { L("v:" + JSON.stringify(v)); };
globalThis.bad = function (e) { L("e:" + (e && e.name) + (e && e.name === "Error" ? ":" + e.message : "")); };
`;

// `catchErrors` (padrão): o harness tem `__final` e `__err`, o corpo vai num `try` que registra o erro síncrono e a
// global `resultName` é um getter de `__final()`. Sem `catchErrors`, `resultName` já é uma global do harness e o programa
// que lança de forma síncrona (o bun sai com código diferente de zero) é descartado.
// `own` é o nome do golden do gerador (`x_bun.tsv`), excluído de `knownBodies`; obrigatório para quem usa `knownBodies`.
function asyncGolden({ harness = DEFAULT_HARNESS, resultName = "R", catchErrors = true, own } = {}) {
  const prelude = catchErrors
    ? harness.trimEnd() + "\nObject.defineProperty(globalThis, " + JSON.stringify(resultName) + ", { get: function () { return __final(); }, configurable: true });\n"
    : harness;

  // O arquivo do caso: o prelúdio do harness e o corpo (uma linha, sem tab), com `try` quando `catchErrors`.
  function originalProgram(body) {
    if (!catchErrors) return prelude + body;
    return prelude + BODY_START + body + BODY_END + "\"error\\t\" + e.name + \"\\t\" + JSON.stringify(String(e.message)); }\n";
  }

  // Conjunto dos corpos já presentes nos goldens vizinhos: `has(originalProgram(body))`.
  function knownBodies(filter) {
    return knownProgramSet(own, filter, bodyKey);
  }

  // Executa os corpos no bun, cada um em processos próprios (`jobs` ao mesmo tempo), `runs` vezes: saídas que diferem
  // entre as corridas descartam o programa (não determinístico no bun). O arquivo se chama `fileName` dos dois lados
  // (o teste usa o mesmo nome). Devolve as linhas `JSON(programa)<TAB>JSON(resultado)[<TAB>JSON(meta)]` para
  // `emitFactoredLines`, na ordem dos corpos.
  async function measureBodies(bodies, fileName, { jobs = 8, runs = 1, swallowUncaught = false } = {}) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "async-golden-"));
    const preload = path.join(dir, "preload.js");
    fs.writeFileSync(
      preload,
      "process.on('unhandledRejection', () => {});\n" + (swallowUncaught ? "process.on('uncaughtException', () => {});\n" : "") +
        "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis[" +
        JSON.stringify(resultName) + "] === undefined ? '<undefined>' : String(globalThis[" + JSON.stringify(resultName) + "])) + '\\n') })\n",
    );
    const lines = new Array(bodies.length).fill(null);
    let next = 0;
    let finished = 0;
    let unstable = 0;
    let leaked = 0;
    let thrown = 0;
    const runOne = async (index) => {
      const body = bodies[index];
      if (/[\t\n\r]/.test(body)) throw new Error("fonte com tab ou quebra de linha: " + body);
      const { source, executable, meta } = prepareProgram(originalProgram(body));
      const caseDir = path.join(dir, "c" + index);
      fs.mkdirSync(caseDir);
      const file = path.join(caseDir, fileName);
      fs.writeFileSync(file, executable);
      const results = new Set();
      for (let k = 0; k < runs; k++) {
        const run = await spawnOnce(process.execPath, ["--preload", preload, file], caseDir, 20000);
        const marked = run.out.split("\n").find((line) => line.startsWith("\u0001"));
        if (!marked) throw new Error("sem resultado do bun para: " + body + "\n" + run.err);
        if (run.code !== 0 && !swallowUncaught) {
          thrown++;
          return;
        }
        results.add(JSON.parse(marked.slice(1)));
      }
      if (results.size > 1) {
        unstable++;
        return;
      }
      const [result] = results;
      if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
        leaked++;
        return;
      }
      lines[index] = JSON.stringify(source) + "\t" + JSON.stringify(result) + (meta ? "\t" + JSON.stringify(meta) : "");
    };
    const worker = async () => {
      while (next < bodies.length) {
        await runOne(next++);
        finished++;
        if (finished % 100 === 0 || finished === bodies.length) process.stderr.write(`medidos ${finished}/${bodies.length}\n`);
      }
    };
    try {
      await Promise.all(Array.from({ length: jobs }, worker));
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
    if (unstable) process.stderr.write(`descartados ${unstable} programas não determinísticos no bun\n`);
    if (leaked) process.stderr.write(`descartados ${leaked} programas com caminho da máquina no resultado\n`);
    if (thrown) process.stderr.write(`descartados ${thrown} programas que lançam de forma síncrona\n`);
    return lines.filter((line) => line !== null);
  }

  return { originalProgram, knownBodies, measureBodies };
}

module.exports = { asyncGolden, bodyKey, ORDER_HARNESS };
