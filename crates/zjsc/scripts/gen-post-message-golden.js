// Gera tests/golden/post_message_bun.tsv: `postMessage` do global medido no bun 1.4.2 (descritor, `length`, `name`,
// chaves próprias, `toString`, `new`, argumentos de qualquer tipo, `this` alheio, retorno, ordem de chaves, atribuição
// e `delete`). Na thread principal a função não faz nada observável e devolve `undefined`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON). Um processo `bun` por linha.
// Uso: bun scripts/gen-post-message-golden.js > tests/golden/post_message_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");
const { runMain } = require("./uncaught-run.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v === undefined) return 'undefined'; if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message };\n" +
  "var D = function (o, k) { var x = Object.getOwnPropertyDescriptor(o, k); return x && [typeof x.value, x.writable, x.enumerable, x.configurable, typeof x.get, typeof x.set] };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const stmts = (code) => programs.push(HELPER + `try { ${code} } catch (e) { R = E(e) }`);
// Caso cuja exceção escapa para o laço de eventos: roda como `/app/main.js` e o golden próprio guarda o stderr inteiro
// e o código de saída (`tests/golden/post_message_uncaught_bun.tsv`), como `gen-uncaught-golden.js`.
const uncaughtSources = [];
const uncaught = (code) => uncaughtSources.push(HELPER + `try { ${code} } catch (e) { R = E(e) }`);
const strict = (code) => programs.push(HELPER + `try { (function () { 'use strict'; ${code} })() } catch (e) { R = E(e) }`);
const P = "Object.getOwnPropertyDescriptor(globalThis, 'postMessage')";
const names = "Object.getOwnPropertyNames(globalThis)";

// Descritor, forma da função.
expr(`D(globalThis, 'postMessage')`);
expr("typeof postMessage");
expr("postMessage.length");
expr("postMessage.name");
expr("Object.getOwnPropertyNames(postMessage)");
expr("Reflect.ownKeys(postMessage).length");
expr("String(postMessage)");
expr("postMessage.toString()");
expr("'prototype' in postMessage");
expr("Object.getPrototypeOf(postMessage) === Function.prototype");
expr("D(postMessage, 'name')");
expr("D(postMessage, 'length')");
expr("Object.prototype.toString.call(postMessage)");
expr("Object.keys(globalThis).indexOf('postMessage') >= 0");
expr("globalThis.postMessage === postMessage");
expr("self.postMessage === postMessage");
expr("Object.prototype.hasOwnProperty.call(globalThis, 'postMessage')");

// Construtor.
expr("new postMessage()");
expr("new postMessage(1)");
expr("Reflect.construct(postMessage, [])");
expr("Reflect.construct(function () {}, [], postMessage)");

// Retorno e argumentos de qualquer tipo.
expr("postMessage()");
expr("postMessage(undefined)");
expr("postMessage(null)");
expr("postMessage(1)");
expr("postMessage('x')");
expr("postMessage({})");
expr("postMessage([1, 2, 3])");
expr("postMessage(1n)");
expr("postMessage(Symbol('s'))");
expr("postMessage(function () {})");
expr("postMessage(function () {}, [])");
expr("postMessage(new Date(), { transfer: 5 })");
expr("postMessage(1, 2, 3, 4, 5)");
expr("postMessage(1, 'origin')");
expr("postMessage('x', '*')");
expr("postMessage('x', { targetOrigin: '*' })");
expr("postMessage(new ArrayBuffer(8), [])");
stmts("var a = new ArrayBuffer(8); var r = postMessage(a, [a]); R = S([r, a.byteLength, a.detached])");
stmts("var c = {}; c.c = c; R = S(postMessage(c))");
stmts("var n = 0; var o = { get a() { n++; return 1 } }; postMessage(o); R = S(n)");
stmts("postMessage({ get x() { throw new Error('boom') } }); R = 'sem erro'");
stmts("var order = []; postMessage({ toString() { order.push('t'); return '' } }, { get transfer() { order.push('g'); return [] } }); R = S(order)");
stmts("postMessage(new Proxy({}, { get() { throw new Error('trap') }, ownKeys() { throw new Error('trap') } })); R = 'sem erro'");

// `this` alheio.
expr("postMessage.call(null, 1)");
expr("postMessage.call(undefined, 1)");
expr("postMessage.call({}, 1)");
expr("postMessage.call(5, 1)");
expr("postMessage.call('x')");
expr("postMessage.apply(null, [1, 2])");
expr("postMessage.bind(null, 1)()");
expr("Reflect.apply(postMessage, {}, [])");
expr("[1, 2].map(postMessage)");
expr("(0, postMessage)(1)");
expr("(function () { return postMessage(1) })()");
expr("(function () { 'use strict'; return postMessage.call(undefined) })()");

// Ordem de chaves.
expr(`${names}.indexOf('postMessage') - ${names}.indexOf('fetch')`);
expr(`${names}.indexOf('prompt') - ${names}.indexOf('postMessage')`);
expr(`${names}.indexOf('postMessage') > ${names}.indexOf('dispatchEvent') && ${names}.indexOf('postMessage') < ${names}.indexOf('queueMicrotask')`);
expr(`${names}.indexOf('postMessage') < ${names}.indexOf('removeEventListener')`);
expr(`${names}.indexOf('postMessage') < ${names}.indexOf('structuredClone')`);

// Atribuição, redefinição e delete.
stmts("postMessage = 5; R = S([typeof postMessage, D(globalThis, 'postMessage')])");
strict("postMessage = 5; R = S([typeof postMessage, D(globalThis, 'postMessage')])");
stmts("var i = " + names + ".indexOf('postMessage'); postMessage = 5; R = S(" + names + ".indexOf('postMessage') - i)");
stmts("R = S([delete globalThis.postMessage, typeof postMessage, 'postMessage' in globalThis, D(globalThis, 'postMessage')])");
stmts("delete globalThis.postMessage; try { postMessage(1) } catch (e) { R = E(e) }");
strict("delete globalThis.postMessage; postMessage(1)");
stmts("delete globalThis.postMessage; postMessage = 3; R = S([typeof postMessage, D(globalThis, 'postMessage')])");
stmts("var f = postMessage; delete globalThis.postMessage; R = S([f(1), f.name, f.length])");
stmts("Object.defineProperty(globalThis, 'postMessage', { value: 7 }); R = S([postMessage, D(globalThis, 'postMessage')])");
stmts("Object.defineProperty(globalThis, 'postMessage', { enumerable: false }); R = S([typeof postMessage, D(globalThis, 'postMessage')])");
stmts("postMessage.extra = 1; R = S([postMessage.extra, Object.keys(postMessage)])");
stmts("postMessage.name = 'x'; R = S(postMessage.name)");
strict("postMessage.name = 'x'");
strict("postMessage.length = 9");
stmts("delete postMessage.name; R = S([postMessage.name, Object.getOwnPropertyNames(postMessage)])");
expr("Object.isExtensible(postMessage)");
expr("Object.isFrozen(postMessage)");

// `onmessage` e `addEventListener("message")` no global. Medido no bun 1.4.2: `onmessage` é propriedade de dados comum
// (valor `null`, writable, enumerable, configurable, sem getter/setter), logo depois de `onerror` na ordem de chaves;
// `postMessage` na thread principal não despacha nada; `dispatchEvent(new MessageEvent(...))` chama o `onmessage` e
// depois os ouvintes, de forma síncrona (data, origin "", source null, ports vazio). Atribuir função a `onmessage` ou
// manter ouvinte de "message" mantém o processo vivo, então os casos desfazem ambos antes de terminar.
expr("D(globalThis, 'onmessage')");
expr("globalThis.onmessage");
expr("typeof onmessage");
expr("'onmessage' in globalThis");
expr("Object.keys(globalThis).slice(-2)");
expr("Object.keys(globalThis).indexOf('onmessage') - Object.keys(globalThis).indexOf('onerror')");
expr("self.onmessage === onmessage");
stmts("var log = []; onmessage = function (e) { log.push('om:' + e.data) }; postMessage('x'); log.push('after'); onmessage = null; R = S(log)");
stmts("var log = []; var f = function (e) { log.push('l:' + e.data) }; addEventListener('message', f); postMessage('x'); log.push('after'); removeEventListener('message', f); R = S(log)");
stmts("var log = []; var f = function (e) { log.push(['l', e.type, e.data, e.origin, e.source, e.ports.length, e.target === globalThis, e.currentTarget === globalThis, this === globalThis]) }; addEventListener('message', f); dispatchEvent(new MessageEvent('message', { data: 1 })); removeEventListener('message', f); R = S(log)");
stmts("var log = []; var f = function (e) { log.push('l') }; onmessage = function (e) { log.push('om') }; addEventListener('message', f); dispatchEvent(new MessageEvent('message', { data: 1 })); onmessage = null; removeEventListener('message', f); R = S(log)");
stmts("var log = []; var f = function (e) { log.push('l') }; addEventListener('message', f); onmessage = function (e) { log.push('om') }; dispatchEvent(new MessageEvent('message')); onmessage = null; removeEventListener('message', f); R = S(log)");
stmts("var log = []; onmessage = function (e) { log.push([e.data, e.origin, e.lastEventId, e.source, e.ports.length]) }; dispatchEvent(new MessageEvent('message', { data: { a: 1 }, origin: 'o', lastEventId: 'i' })); onmessage = null; R = S(log)");
stmts("var log = []; onmessage = 5; dispatchEvent(new MessageEvent('message')); R = S([typeof onmessage, onmessage]); onmessage = null");
stmts("var log = []; onmessage = function () { log.push('om') }; dispatchEvent(new Event('message')); dispatchEvent(new Event('messageerror')); onmessage = null; R = S(log)");
stmts("var log = []; onmessage = function () { log.push('om'); Promise.resolve().then(function () { log.push('micro') }); log.push('end') }; dispatchEvent(new MessageEvent('message')); log.push('sync'); onmessage = null; R = S(log)");
stmts("var o = function () {}; onmessage = o; var same = onmessage === o; onmessage = null; R = S([same, onmessage])");
stmts("var n = 0; var f = function () { n++ }; addEventListener('message', f); addEventListener('message', f); dispatchEvent(new MessageEvent('message')); removeEventListener('message', f); R = S(n)");
stmts("var log = []; var f = function (e) { log.push('once'); }; addEventListener('message', f, { once: true }); dispatchEvent(new MessageEvent('message')); dispatchEvent(new MessageEvent('message')); R = S(log)");
// Erro lançado pelo `onmessage` vira exceção não capturada: o `dispatchEvent` retorna, o script segue e o bun sai com 1.
// O resultado desses casos fica em `post_message_uncaught_bun.tsv`, não no golden principal.
uncaught("onmessage = function (e) { throw new Error('boom') }; dispatchEvent(new MessageEvent('message')); onmessage = null; R = 'depois'");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "pm-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `(0, eval)("var R");\n(0, eval)(${JSON.stringify(sourceAscii)});\nprocess.stdout.write(JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
  );
  const run = spawnSync(process.execPath, [file], { encoding: "utf8", timeout: 15000 });
  fs.rmSync(dir, { recursive: true, force: true });
  if (run.status !== 0) throw new Error("bun falhou em: " + sourceAscii + "\n" + run.stderr);
  emitRow(JSON.stringify(sourceAscii) + "\t" + run.stdout);
}

// Os casos de exceção não capturada vão para o golden próprio, no formato de `uncaught_bun.tsv` (stderr inteiro em
// hexadecimal e código de saída), rodados como `/app/main.js`.
fs.writeFileSync(path.join(__dirname, "..", "tests", "golden", "post_message_uncaught_bun.tsv"), uncaughtSources.map((c) => runMain(c)).join("\n") + "\n");
