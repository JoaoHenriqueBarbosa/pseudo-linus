// Gera tests/golden/message_channel_loop_bun.tsv: a grade que mede se `MessageChannel`/`MessagePort` seguram o laço de
// eventos no bun 1.4.2. Cada programa roda como `main.js`, sem timer nenhum que feche as portas, com limite curto de
// tempo (`TIMEOUT_MS`). O resultado observável é a saída, o código de saída e se o processo ainda estava vivo ao estourar
// o limite (`1` na última coluna): vivo é o laço segurado por alguma porta; `0` é o processo que terminou sozinho.
// Formato da linha: o de `uncaught-run.js` com stdout e `reportAlive` (fonte JSON, stderr hex, saída, stdout hex, vivo).
// Uso: bun scripts/gen-message-channel-loop-golden.js > tests/golden/message_channel_loop_bun.tsv
const { runMain } = require("./uncaught-run.js");
const { emitRow } = require("./golden-prelude.js");

const TIMEOUT_MS = 1500;
const HEAD = "var m = new MessageChannel(), a = m.port1, b = m.port2, n = new MessageChannel(), f = function () {};\n";
const bodies = [
  "", "b.onmessage = f;", "b.addEventListener('message', f);", "b.addEventListener('message', f); b.start();",
  "a.onmessage = f; b.onmessage = f;", "a.onmessage = f; b.addEventListener('message', f);",
  "a.addEventListener('message', f); b.addEventListener('message', f);",
  "a.addEventListener('message', f); b.addEventListener('message', f); a.start(); b.start();",
  "b.onmessage = f; a.postMessage(1);", "b.addEventListener('message', f); a.postMessage(1);",
  "b.onmessage = f; b.postMessage(1);", "a.postMessage(1);", "b.onmessage = f; b.ref();", "b.onmessage = f; b.hasRef();",
  "b.onmessage = f; b.ref(); b.unref();", "b.onmessage = f; b.unref();", "b.onmessage = f; a.postMessage(1); b.unref();",
  "b.addEventListener('message', f); b.ref();", "b.ref();", "b.hasRef();", "b.ref(); b.onmessage = f;", "b.onmessage = f; b.ref();",
  "b.ref(); a.onmessage = f;", "b.ref(); b.onmessage = f; a.onmessage = f;", "b.ref(); b.unref();", "b.ref(); b.close();",
  "b.ref(); a.close();", "b.ref(); b.addEventListener('message', f);", "b.ref(); b.ref(); b.unref();", "a.ref(); b.ref();",
  "a.ref(); b.ref(); a.unref();", "b.ref(); a.postMessage(1);", "b.ref(); b.onmessage = f; a.postMessage(1);",
  "a.onmessage = f; b.onmessage = f; b.unref();", "a.onmessage = f; b.onmessage = f; a.unref();",
  "a.onmessage = f; b.onmessage = f; a.unref(); b.unref();", "a.onmessage = f; b.onmessage = f; a.close();",
  "a.onmessage = f; b.onmessage = f; b.close();", "b.onmessage = f; a.postMessage(1); b.close();",
  "b.onmessage = f; a.postMessage(1); a.close();", "b.onmessage = f; b.onmessage = null;",
  "a.onmessage = f; b.onmessage = f; b.onmessage = null;", "b.onmessage = f; a.postMessage(1); b.onmessage = null;",
  "a.addEventListener('message', f); b.addEventListener('message', f); b.removeEventListener('message', f);",
  "b.onmessageerror = f;", "a.onmessageerror = f; b.onmessageerror = f;", "b.addEventListener('message', f, { once: true }); b.start();",
  "b.onmessage = f; a.postMessage(1, [n.port2]);",
  "b.onmessage = function (e) { e.ports[0].onmessage = f }; a.postMessage(1, [n.port2]);",
  "b.onmessage = function (e) { e.ports[0].onmessage = f; n.port1.onmessage = f }; a.postMessage(1, [n.port2]);",
  "n.port1.onmessage = f; n.port2.onmessage = f; a.postMessage(1, [n.port2]);", "n.port1.onmessage = f; a.postMessage(1, [n.port2]);",
  "n.port1.onmessage = f; n.port2.onmessage = f;", "b.onmessage = f; n.port2.onmessage = f;",
  "a.postMessage(1); setTimeout(function () { b.onmessage = f }, 10);", "a.postMessage(1); b.onmessage = f;",
  "b.onmessage = f; a.postMessage(1); b.postMessage(2);", "b.onmessage = f; b.ref(); a.close();", "b.onmessage = f; b.ref(); a.onmessage = f;",
  "a.onmessage = f; b.onmessage = f; b.ref(); a.unref();", "b.onmessage = f; b.ref(); a.unref();",
];
for (const body of bodies) emitRow(runMain(HEAD + body + "\n", true, { timeoutMs: TIMEOUT_MS, reportAlive: true }));
