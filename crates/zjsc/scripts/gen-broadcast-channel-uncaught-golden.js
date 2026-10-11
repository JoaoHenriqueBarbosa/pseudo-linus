// Gera tests/golden/broadcast_channel_uncaught_bun.tsv: o que o bun 1.4.2 faz quando o handler de um `BroadcastChannel`
// lança. Cada fonte roda como `/app/main.js` (`scripts/uncaught-run.js`) e a linha guarda o stderr inteiro, o código de
// saída e o stdout, no formato de `uncaught_bun.tsv` (lado Rust: `common::MainScriptRow`). Todo programa fecha ou dá
// `unref` nos canais: canal aberto com `ref` segura o processo vivo para sempre e o teste não pode travar.
// Uso: bun scripts/gen-broadcast-channel-uncaught-golden.js > tests/golden/broadcast_channel_uncaught_bun.tsv
const { runMain } = require("./uncaught-run.js");

const sources = [
  "var a = new BroadcastChannel('x'), b = new BroadcastChannel('x'); b.onmessage = function (e) { throw new Error('boom') }; a.postMessage(1); setTimeout(function () { a.close(); b.close(); console.log('fim') }, 20)",
  "var a = new BroadcastChannel('x'), b = new BroadcastChannel('x'); b.addEventListener('message', function (e) { throw new TypeError('listener') }); a.postMessage(1); setTimeout(function () { a.close(); b.close(); console.log('fim') }, 20)",
  "var a = new BroadcastChannel('x'), b = new BroadcastChannel('x'); b.onmessage = function (e) { throw 42 }; a.postMessage(1); setTimeout(function () { a.close(); b.close(); console.log('fim') }, 20)",
  "var a = new BroadcastChannel('x'), b = new BroadcastChannel('x'); b.onmessage = function (e) { console.log('got ' + e.data); if (e.data === 1) throw new Error('first') }; a.postMessage(1); a.postMessage(2); setTimeout(function () { a.close(); b.close(); console.log('fim') }, 20)",
  "var a = new BroadcastChannel('x'), b = new BroadcastChannel('x'), c = new BroadcastChannel('x'); b.onmessage = function (e) { throw new Error('b') }; c.onmessage = function (e) { console.log('c ' + e.data) }; a.postMessage(1); setTimeout(function () { a.close(); b.close(); c.close(); console.log('fim') }, 20)",
  "var a = new BroadcastChannel('x'), b = new BroadcastChannel('x'); b.onmessage = function (e) { throw new Error('boom') }; process.on('uncaughtException', function (err) { console.log('caught ' + err.message); a.close(); b.close() }); a.postMessage(1)",
];

process.stdout.write(sources.map((source) => runMain(source, true)).join("\n") + "\n");
