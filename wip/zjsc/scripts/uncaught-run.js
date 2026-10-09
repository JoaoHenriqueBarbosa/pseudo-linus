// Roda um programa como `main.js` no bun e devolve a linha do golden de exceção não capturada:
// `JSON(fonte)<TAB>hex(stderr normalizado)<TAB>código de saída`. O caminho do diretório temporário vira `/app` no stderr.
// Compartilhado por `gen-uncaught-golden.js` e `gen-post-message-golden.js`; o lado Rust é `common::check_main_script_row`.
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

// Com `withStdout`, a linha ganha uma quarta coluna: o hex do stdout normalizado do mesmo jeito.
// `options.timeoutMs` troca o limite de tempo (padrão 20 s). Com `options.reportAlive`, a linha ganha ainda uma coluna
// final: `1` se o processo estava vivo ao estourar o limite (o laço de eventos segurado), `0` se terminou sozinho.
function runMain(source, withStdout = false, options = {}) {
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "uncaught-")));
  try {
    const file = path.join(dir, "main.js");
    fs.writeFileSync(file, source);
    const r = spawnSync(process.execPath, [file], { cwd: dir, env: { PATH: process.env.PATH, HOME: dir, NO_COLOR: "1" }, stdio: ["ignore", withStdout ? "pipe" : "ignore", "pipe"], timeout: options.timeoutMs || 20000 });
    const normalize = (buffer) => Buffer.from(buffer.toString("latin1").split(dir + "/main.js").join("/app/main.js").split(dir).join("/app"), "latin1").toString("hex");
    const stdoutColumn = withStdout ? `\t${normalize(r.stdout)}` : "";
    const aliveColumn = options.reportAlive ? `\t${r.error && r.error.code === "ETIMEDOUT" ? 1 : 0}` : "";
    return `${JSON.stringify(source)}\t${normalize(r.stderr)}\t${r.status === null ? "signal:" + r.signal : r.status}${stdoutColumn}${aliveColumn}`;
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

module.exports = { runMain };
