// Gera tests/golden/broadcast_channel_delivery_bun.tsv: a entrega do `BroadcastChannel` medida no bun 1.4.2.
// Cobre a entrega assíncrona entre canais do mesmo nome, a ordem relativa a microtasks e timers, o remetente que
// nunca recebe a própria mensagem, canais de nomes diferentes, o `MessageEvent` entregue (tipo, `data`, `origin`,
// `lastEventId`, `source`, `ports`, `target`), `onmessage` e `addEventListener`, `close` no meio da entrega e
// no destinatário, o clone estruturado da mensagem (cópia por canal, `Map`, `Date`, referência circular) e `ref`/`unref`.
// Cada programa registra a ordem dos acontecimentos em `LOG` e grava o texto final em `R`; um temporizador de
// 20 ms fecha todos os canais, porque canal aberto segura o laço de eventos (um processo `bun` por programa).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-broadcast-channel-delivery-golden.js > tests/golden/broadcast_channel_delivery_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var LOG = [];\n" +
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; if (v === undefined) return 'undefined'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code };\n" +
  "var ALL = [];\n" +
  "var mk = function (n) { var c = new BroadcastChannel(n); ALL.push(c); return c };\n" +
  "var done = function () { setTimeout(function () { ALL.forEach(function (c) { c.close() }); R = LOG.join(',') }, 20) };\n";
const programs = [];
const run = (code) => programs.push(HELPER + `try { ${code}; done() } catch (e) { R = E(e) }`);

// Entrega básica: assíncrona, nunca ao remetente.
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.onmessage = function (e) { LOG.push('a:' + S(e.data)) }; a.postMessage(1); LOG.push('sync')");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage('s'); b.postMessage('t'); a.onmessage = function (e) { LOG.push('a:' + S(e.data)) }");
run("var a = mk('x'), b = mk('y'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1)");
run("var a = mk('x'), b = mk('x'), c = mk('x'); [b, c].forEach(function (ch, i) { ch.onmessage = function (e) { LOG.push('ch' + i + ':' + S(e.data)) } }); a.postMessage(7)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1); a.postMessage(2); a.postMessage(3)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage('m'); Promise.resolve().then(function () { LOG.push('micro') }); queueMicrotask(function () { LOG.push('qm') }); setTimeout(function () { LOG.push('t0') }, 0); setImmediate(function () { LOG.push('imm') }); process.nextTick(function () { LOG.push('tick') })");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; setTimeout(function () { LOG.push('t0') }, 0); a.postMessage('m'); setTimeout(function () { LOG.push('t1') }, 1)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)); Promise.resolve().then(function () { LOG.push('micro-in-b') }) }; a.postMessage(1); a.postMessage(2)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)); b.postMessage('echo') }; a.onmessage = function (e) { LOG.push('a:' + S(e.data)) }; a.postMessage('ping')");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)); if (e.data < 3) b.postMessage(e.data + 1) }; a.onmessage = function (e) { LOG.push('a:' + S(e.data)); a.postMessage(e.data + 10) }; a.postMessage(1)");

// O MessageEvent entregue.
const ev = (props) => run(`var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push(S(${props})) }; a.postMessage({ k: 1 })`);
ev("e.type");
ev("[e instanceof MessageEvent, e instanceof Event, e.constructor === MessageEvent]");
ev("[e.origin, e.lastEventId, e.source, e.ports, e.ports.length]");
ev("[e.target === b, e.currentTarget === b, e.srcElement === b, e.eventPhase, e.bubbles, e.cancelable, e.composed, e.isTrusted, e.defaultPrevented]");
ev("[typeof e.timeStamp, e.timeStamp >= 0]");
ev("e.data");
ev("Object.prototype.toString.call(e)");
ev("this === b");
ev("Object.keys(e)");

// addEventListener, removeEventListener, once, handler objeto, onmessage e addEventListener juntos.
run("var a = mk('x'), b = mk('x'); b.addEventListener('message', function (e) { LOG.push('l1:' + S(e.data)) }); b.onmessage = function (e) { LOG.push('on:' + S(e.data)) }; b.addEventListener('message', function (e) { LOG.push('l2:' + S(e.data)) }); a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); var f = function (e) { LOG.push('f:' + S(e.data)) }; b.addEventListener('message', f); b.removeEventListener('message', f); a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.addEventListener('message', function (e) { LOG.push('once:' + S(e.data)) }, { once: true }); a.postMessage(1); a.postMessage(2)");
run("var a = mk('x'), b = mk('x'); b.addEventListener('message', { handleEvent: function (e) { LOG.push('obj:' + S(e.data)) } }); a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('first') }; b.onmessage = function (e) { LOG.push('second') }; a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('x') }; b.onmessage = null; a.postMessage(1); LOG.push('end')");
run("var a = mk('x'), b = mk('x'); b.addEventListener('message', function (e) { e.stopImmediatePropagation(); LOG.push('stop') }); b.addEventListener('message', function (e) { LOG.push('second') }); a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.onmessageerror = function (e) { LOG.push('err') }; b.onmessage = function (e) { LOG.push('msg:' + S(e.data)) }; a.postMessage(1)");

// Clone estruturado: cópia por canal, tipos, referência circular, função, símbolo.
run("var a = mk('x'), b = mk('x'), c = mk('x'); var o = { n: 1 }; var got = []; b.onmessage = function (e) { got.push(e.data) }; c.onmessage = function (e) { got.push(e.data) }; a.postMessage(o); setTimeout(function () { LOG.push([got.length, got[0] === o, got[1] === o, got[0] === got[1], S(got[0])].join(';')) }, 5)");
run("var a = mk('x'), b = mk('x'); var o = { n: 1 }; b.onmessage = function (e) { LOG.push(S(e.data)) }; a.postMessage(o); o.n = 2");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push(Object.prototype.toString.call(e.data) + ':' + (e.data instanceof Map) + ':' + e.data.get('k')) }; a.postMessage(new Map([['k', 'v']]))");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push((e.data instanceof Date) + ':' + e.data.getTime()) }; a.postMessage(new Date(86400000))");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push(S(e.data.self === e.data)) }; var o = {}; o.self = o; a.postMessage(o)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push(S([e.data.constructor === Uint8Array, Array.from(e.data)])) }; a.postMessage(new Uint8Array([1, 2, 3]))");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push(S(e.data)) }; a.postMessage(undefined); a.postMessage(null); a.postMessage('')");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push(S([e.data instanceof Error, e.data.message, e.data.name])) }; a.postMessage(new RangeError('r'))");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('got') }; try { a.postMessage(function () {}) } catch (e) { LOG.push(E(e)) }");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('got') }; try { a.postMessage(Symbol()) } catch (e) { LOG.push(E(e)) }");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('got:' + S(e.data)) }; try { a.postMessage({ get x() { throw new Error('getter') } }) } catch (e) { LOG.push(E(e)) } a.postMessage(2)");

// Nome do canal: conversão com ToString, canais com nomes iguais depois da conversão, nome vazio.
run("var a = mk(1), b = mk('1'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage('n')");
run("var a = mk(''), b = mk(''); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage('empty')");
run("var a = mk('X'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage('case')");
run("var a = mk(undefined), b = mk('undefined'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage('u')");

// close: antes da entrega, no destinatário, no remetente, no meio do lote, dentro do handler.
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1); b.close()");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1); a.close()");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)); b.close() }; a.postMessage(1); a.postMessage(2); a.postMessage(3)");
run("var a = mk('x'), b = mk('x'), c = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)); c.close() }; c.onmessage = function (e) { LOG.push('c:' + S(e.data)) }; a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.close(); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.close(); try { a.postMessage(1) } catch (e) { LOG.push(E(e)) } b.postMessage(2)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1); setTimeout(function () { b.close(); a.postMessage(2) }, 5)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; b.close(); var c = mk('x'); c.onmessage = function (e) { LOG.push('c:' + S(e.data)) }; a.postMessage(1)");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1); a.close(); b.close(); LOG.push('closed')");
run("var a = mk('x'); a.close(); var b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; var c = mk('x'); c.postMessage(1)");

// Canal criado depois do postMessage não recebe o que já saiu; lista de destinatários é fixada no envio.
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1); var late = mk('x'); late.onmessage = function (e) { LOG.push('late:' + S(e.data)) }");
run("var a = mk('x'), b = mk('x'); b.onmessage = function (e) { LOG.push('b:' + S(e.data)); var d = mk('x'); d.onmessage = function (e2) { LOG.push('d:' + S(e2.data)) } }; a.postMessage(1); setTimeout(function () { a.postMessage(2) }, 5)");

// Handler atribuído depois da chegada (antes de o laço entregar) e depois da entrega.
run("var a = mk('x'), b = mk('x'); a.postMessage(1); setTimeout(function () { b.onmessage = function (e) { LOG.push('late-handler:' + S(e.data)) } }, 5)");
run("var a = mk('x'), b = mk('x'); a.postMessage(1); b.onmessage = function (e) { LOG.push('same-tick:' + S(e.data)) }");

// ref / unref: devolvem o próprio canal? o laço termina com canal aberto e unref?
run("var a = mk('x'); LOG.push(a.ref() === a); LOG.push(a.unref() === a); LOG.push(typeof a.ref())");
run("var a = mk('x'), b = mk('x'); a.unref(); b.unref(); b.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; a.postMessage(1)");

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bc-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `process.on("uncaughtException", () => {});\nprocess.on("exit", () => require("fs").writeSync(1, JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R))));\n(0, eval)("var R");\n(0, eval)(${JSON.stringify(sourceAscii)});\n`,
  );
  const result = spawnSync(process.execPath, [file], { encoding: "utf8", timeout: 10000 });
  fs.rmSync(dir, { recursive: true, force: true });
  if (result.status !== 0) throw new Error("bun falhou em: " + sourceAscii + "\n" + result.stderr);
  emitRow(JSON.stringify(sourceAscii) + "\t" + result.stdout);
}
