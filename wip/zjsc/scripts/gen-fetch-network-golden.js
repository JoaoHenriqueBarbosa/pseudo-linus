// Gera tests/golden/fetch_network_bun.tsv: o `fetch` global de `http:`/`https:` medido no bun 1.4.2 numa máquina SEM rede
// (`unshare -rn`, namespace de rede vazio: sem rota, sem `lo` ativo, sem resolvedor), mais o quirk de `file:` de caminho de
// um byte (`FILE:///x` lê a raiz), medido sem rede também. Nenhum pacote sai da máquina.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` como string (JSON). Um processo `bun` por linha,
// rodado como `main.js`; cada rodada roda todos os casos duas vezes e exige saídas idênticas.
// Uso: bun scripts/gen-fetch-network-golden.js > tests/golden/fetch_network_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

// Resumo do erro de uma rejeição: tipo, propriedades próprias (menos as de pilha, que o bun materializa à parte) com
// descritor, protótipo, `String(e)` e a primeira linha da pilha.
const HELPER =
  "var STACK = ['originalLine', 'originalColumn', 'line', 'column', 'sourceURL', 'stack'];\n" +
  "var N = function (e) { var own = Object.getOwnPropertyNames(e).filter(function (k) { return STACK.indexOf(k) < 0 });\n" +
  "  return JSON.stringify({ name: e.name, ctor: e.constructor === TypeError, proto: Object.getPrototypeOf(e) === TypeError.prototype, isError: e instanceof Error, text: String(e), own: own, keys: Object.keys(e), " +
  "desc: own.map(function (k) { var d = Object.getOwnPropertyDescriptor(e, k); return [k, d.value, d.writable, d.enumerable, d.configurable] }), head: e.stack.split('\\n')[0] }) };\n" +
  "var U = function (u) { return fetch(u).then(function () { return 'RESOLVED' }, N) };\n" +
  // Resumo de um `file:` : `url` da resposta e o texto do corpo (ou o `code` do erro de leitura).
  "var FL = async function (u) { try { var r = await fetch(u); var b; try { b = await r.text() } catch (e) { b = e.code } return JSON.stringify([r.status, r.url, b]) } catch (e) { return 'ERR ' + e.name + '|' + e.code + '|' + e.message } };\n";

const programs = [];
const expr = (code) => programs.push(HELPER + `(async function () { try { R = await (${code}) } catch (e) { R = 'ERR ' + e.name + '|' + e.code + '|' + e.message } })()`);

// Nome que não resolve (sem rede): `getaddrinfo ETIMEOUT`, com o host normalizado em `hostname` e a URL normalizada em `path`.
for (const url of [
  "http://example.com/",
  "https://example.com/",
  "http://example.com/p?q=1#h",
  "http://user:pw@example.com/x?y#z",
  "http://EXAMPLE.com:8080/a b",
  "http://example.com./",
  "http://a.invalid/",
  "http://例え.jp/",
  "http://example.com:1/",
  "https://sub.domain.example.org/a/b/c",
]) {
  expr(`U(${JSON.stringify(url)})`);
}
// IP literal: o socket não abre (`FailedToOpenSocket`, sem `syscall` nem `hostname`).
for (const url of [
  "http://127.0.0.1:1/",
  "http://127.0.0.1:1/x?q#h",
  "https://127.0.0.1/",
  "http://10.0.0.1/",
  "http://[::1]:1/",
  "http://0.0.0.0/",
  "http://1/",
  "http://192.168.1.1:8080/a b",
]) {
  expr(`U(${JSON.stringify(url)})`);
}
// `localhost` resolve pelo `hosts` e a conexão é recusada.
for (const url of ["http://localhost:1/", "https://localhost/", "http://localhost/a/b?c#d"]) {
  expr(`U(${JSON.stringify(url)})`);
}
// Sinal já abortado antes de tocar na rede; `Request` como argumento; método e corpo.
expr("(function () { var c = new AbortController(); c.abort(); return fetch('http://example.com/', { signal: c.signal }).then(function () { return 'RESOLVED' }, N) })()");
expr("fetch('http://example.com/', { signal: AbortSignal.abort('why') }).then(function () { return 'RESOLVED' }, function (e) { return typeof e + ':' + e })");
expr("fetch('http://example.com/', { signal: AbortSignal.abort(new RangeError('c')) }).then(function () { return 'RESOLVED' }, N)");
expr("fetch(new Request('http://example.com/x', { method: 'POST', body: 'x' })).then(function () { return 'RESOLVED' }, N)");
expr("fetch('HTTP://Example.COM/').then(function () { return 'RESOLVED' }, N)");
expr("fetch('//example.com/').then(function () { return 'RESOLVED' }, function (e) { return e.name + '|' + e.code + '|' + e.message })");
expr("(function () { var p = fetch('http://example.com/'); return p.then(function () { return 'RESOLVED' }, function (e) { return p instanceof Promise && e instanceof TypeError }) })()");

// Quirk de `file:`: o `pathname` codificado que, sem as barras finais, tem no máximo dois bytes abre a raiz.
for (const url of [
  "FILE:///x",
  "FILE:///",
  "File:///x",
  "fIlE:///x",
  "file:///x",
  "file:///1",
  "file:///a/",
  "file:///a//",
  "file:///x/.",
  "file:///x?q#h",
  "file:///x#",
  "file:///./x",
  "file:///.",
  "file:///..",
  "FILE:///x/",
  "FILE://localhost/x",
  "FILE:/x",
  "FILE:x",
  "FILE:///xy",
  "FILE:///xy/",
  "FILE:///x/y",
  "FILE:///a/b/",
  "FILE:///a/b/c",
  "FILE:///%78",
  "FILE:///%78/",
  "FILE:///x%2Fy",
  "FILE:///a b",
  "FILE:///ü",
  "FILE:///中",
  "file:///%20",
  "file:///x%00",
  "file:///.x",
  "file:///x.",
  "file:///x/../hello",
  "file:///ab/",
]) {
  expr(`FL(${JSON.stringify(url)})`);
}

function runCase(source) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "fn-")));
  try {
    const file = path.join(dir, "main.js");
    fs.writeFileSync(
      file,
      `var R; globalThis.require = require;\nprocess.on("exit", function () { process.stdout.write(JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R))) });\n` +
        `Promise.resolve((0, eval)(${JSON.stringify(sourceAscii)})).then(function () {}, function (e) { globalThis.R = "TOPERR " + e });\n`,
    );
    const env = { PATH: process.env.PATH, HOME: dir, NO_COLOR: "1" };
    // `unshare -rn`: namespace de usuário e de rede novos, sem root; sem rede nenhuma de verdade.
    const run = spawnSync("unshare", ["-rn", process.execPath, file], { cwd: dir, env, encoding: "utf8", timeout: 20000 });
    if (run.status !== 0) throw new Error(`bun falhou (${run.status}) em: ${sourceAscii}\n${run.stderr}`);
    return { sourceAscii, result: run.stdout };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

const round = () => programs.map(runCase);
const first = round();
const second = round();
for (let i = 0; i < first.length; i++) {
  if (first[i].result !== second[i].result) throw new Error(`rodadas divergem em: ${first[i].sourceAscii}\n${first[i].result}\n${second[i].result}`);
}
for (const { sourceAscii, result } of first) emitRow(JSON.stringify(sourceAscii) + "\t" + result);
process.stderr.write(`${first.length} casos\n`);
