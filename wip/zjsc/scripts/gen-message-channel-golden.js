// Gera tests/golden/message_channel_bun.tsv: `MessageChannel` e `MessagePort` medidos no bun 1.4.2. Forma (descritores,
// construtores, erros), `postMessage` com clone e transfer, início implícito por `onmessage` contra `addEventListener`
// (que precisa de `start()`), `close`, ordem de entrega contra microtasks, `process.nextTick`, `setImmediate` e timers,
// porta transferida por outra porta. Cada programa registra a ordem em `LOG` e grava o texto final em `R`; um timer de
// 20 ms fecha todas as portas (as de `ports` dos eventos também), porque um par aberto com ouvintes segura o laço.
// O que segura o laço (medido, não coberto aqui porque o teste não pode esperar um laço parado): um par aberto sem
// `unref` segura quando as duas portas têm ouvinte, ou quando uma mensagem foi postada para uma porta com ouvinte, ou
// depois de `hasRef()` numa porta com ouvinte; fechar uma das duas solta; `onmessage = null` solta.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-message-channel-golden.js > tests/golden/message_channel_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var LOG = [];\n" +
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; if (v === undefined) return 'undefined'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code };\n" +
  "var ALL = [];\n" +
  "var mk = function () { var c = new MessageChannel(); ALL.push(c.port1, c.port2); return c };\n" +
  "var keep = function (e) { e.ports.forEach(function (p) { ALL.push(p) }) };\n" +
  "var done = function () { setTimeout(function () { ALL.forEach(function (p) { p.close() }); R = LOG.join(',') }, 20) };\n";
const programs = [];
const run = (code) => programs.push(HELPER + `try { ${code}; done() } catch (e) { R = E(e) }`);
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) } ALL.forEach(function (p) { p.close() })`);

// Forma.
const C = "MessageChannel";
const P = "MessagePort";
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${C}'))`);
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${P}'))`);
expr(`[${C}.length, ${C}.name, ${P}.length, ${P}.name]`);
expr(`Object.getOwnPropertyNames(${C})`);
expr(`Object.getOwnPropertyNames(${C}.prototype)`);
expr(`Object.getOwnPropertyNames(${P})`);
expr(`Object.getOwnPropertyNames(${P}.prototype)`);
expr(`[Object.getPrototypeOf(${C}) === Function.prototype, Object.getPrototypeOf(${C}.prototype) === Object.prototype, Object.getPrototypeOf(${P}) === EventTarget, Object.getPrototypeOf(${P}.prototype) === EventTarget.prototype]`);
expr(`[${C}.prototype.constructor === ${C}, ${P}.prototype.constructor === ${P}, ${C}.prototype[Symbol.toStringTag], ${P}.prototype[Symbol.toStringTag]]`);
expr(`(function(m){ return [Object.prototype.toString.call(m), Object.prototype.toString.call(m.port1), m.port1 instanceof ${P}, m.port1 instanceof EventTarget, m.port1.constructor === ${P}, m.port1 === m.port2, Object.keys(m), Object.getOwnPropertyNames(m), Object.getOwnPropertyNames(m.port1)] })(mk())`);
for (const p of ["port1", "port2"]) {
  expr(`(function(d){ return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length] })(Object.getOwnPropertyDescriptor(${C}.prototype, '${p}'))`);
}
expr(`(function(m){ return [m.port1 === m.port1, m.port2 === m.port2, m.port1 !== m.port2] })(mk())`);
for (const m of ["postMessage", "start", "close", "ref", "unref", "hasRef"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${P}.prototype, '${m}'))`);
}
for (const p of ["onmessage", "onmessageerror"]) {
  expr(`(function(d){ return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length, d.set && d.set.name, d.set && d.set.length] })(Object.getOwnPropertyDescriptor(${P}.prototype, '${p}'))`);
}
expr(`${C}()`);
expr(`new ${P}()`);
expr(`${P}()`);
expr(`new ${C}(1, 2) instanceof ${C}`);
expr(`${P}.prototype.postMessage.call({}, 1)`);
expr(`${P}.prototype.start.call({})`);
expr(`${P}.prototype.close.call({})`);
expr(`Object.getOwnPropertyDescriptor(${P}.prototype, 'onmessage').get.call({})`);
expr(`Object.getOwnPropertyDescriptor(${P}.prototype, 'onmessage').set.call({}, function () {})`);
expr(`${C}.prototype.port1`);
expr(`Object.getOwnPropertyDescriptor(${C}.prototype, 'port1').get.call({})`);
expr(`(function(m){ return [m.port1.onmessage, m.port1.onmessageerror] })(mk())`);
expr(`(function(m){ var f = function () {}; m.port1.onmessage = f; return [m.port1.onmessage === f, m.port2.onmessage] })(mk())`);
expr(`(function(m){ return [m.port1.postMessage(1), m.port1.start(), m.port1.hasRef(), m.port1.ref(), m.port1.hasRef(), m.port1.unref() === m.port1, m.port1.hasRef(), m.port1.close()] })(mk())`);
expr(`(function(m){ m.port1.close(); return [m.port1.postMessage(1), m.port1.close(), m.port1.start(), m.port1.hasRef()] })(mk())`);
expr(`(function(m){ return m.port1.postMessage() })(mk())`);
expr(`(function(m){ return m.port1.postMessage(function () {}) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(Symbol()) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, 5) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, [1]) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, [{}]) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, [m.port1]) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, { transfer: [m.port1] }) })(mk())`);
expr(`(function(m, n){ m.port1.postMessage(1, [n.port1]); return n.port1.postMessage(2) })(mk(), mk())`);
expr(`(function(m, n){ m.port1.postMessage(1, [n.port1]); return m.port1.postMessage(2, [n.port1]) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage(1, [n.port1, n.port1]) })(mk(), mk())`);
expr(`(function(m){ return m.port1.postMessage(1, { transfer: 5 }) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, { transfer: [] }) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, null) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, undefined) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, new Set()) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(1, [m.port2]) })(mk())`);
expr(`(function(m){ return m.port1.postMessage(m.port2) })(mk())`);
expr(`(function(m){ return m.port1.postMessage({ p: m.port2 }, [m.port2]) })(mk())`);
expr(`String(mk().port1)`);
expr(`String(mk())`);
expr(`(function(m){ return m.port1.postMessage(new ArrayBuffer(4), [new ArrayBuffer(4)]) })(mk())`);

// Entrega: assíncrona, par, nunca à própria porta.
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.onmessage = function (e) { LOG.push('a:' + S(e.data)) }; m.port1.postMessage(1); LOG.push('sync')");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); m.port1.postMessage(2); m.port1.postMessage(3)");
run("var m = mk(); m.port1.onmessage = function (e) { LOG.push('a:' + S(e.data)) }; m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage('x'); m.port2.postMessage('y')");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage('m'); Promise.resolve().then(function () { LOG.push('micro') }); queueMicrotask(function () { LOG.push('qm') }); setTimeout(function () { LOG.push('t0') }, 0); setImmediate(function () { LOG.push('imm') }); process.nextTick(function () { LOG.push('tick') })");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; setTimeout(function () { LOG.push('t0') }, 0); m.port1.postMessage('m'); setTimeout(function () { LOG.push('t1') }, 1)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)); Promise.resolve().then(function () { LOG.push('micro-in-b') }) }; m.port1.postMessage(1); m.port1.postMessage(2)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)); m.port2.postMessage('echo') }; m.port1.onmessage = function (e) { LOG.push('a:' + S(e.data)) }; m.port1.postMessage('ping')");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)); if (e.data < 3) m.port2.postMessage(e.data + 1) }; m.port1.onmessage = function (e) { LOG.push('a:' + S(e.data)); m.port1.postMessage(e.data + 10) }; m.port1.postMessage(1)");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { LOG.push('m:' + S(e.data)) }; n.port2.onmessage = function (e) { LOG.push('n:' + S(e.data)) }; m.port1.postMessage(1); n.port1.postMessage(2); m.port1.postMessage(3)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); setTimeout(function () { m.port1.postMessage(2) }, 5)");

// Início implícito: onmessage inicia, addEventListener não (precisa de start()).
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('l:' + S(e.data)) }); m.port1.postMessage(1)");
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('l:' + S(e.data)) }); m.port1.postMessage(1); m.port2.start()");
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('l:' + S(e.data)) }); m.port1.postMessage(1); setTimeout(function () { LOG.push('late-start'); m.port2.start() }, 5)");
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('l:' + S(e.data)) }); m.port2.start(); m.port1.postMessage(1); m.port1.postMessage(2)");
run("var m = mk(); m.port1.postMessage(1); m.port1.postMessage(2); setTimeout(function () { m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) } }, 5)");
run("var m = mk(); m.port1.postMessage(1); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; LOG.push('sync')");
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('l1:' + S(e.data)) }); m.port2.onmessage = function (e) { LOG.push('on:' + S(e.data)) }; m.port2.addEventListener('message', function (e) { LOG.push('l2:' + S(e.data)) }); m.port1.postMessage(1)");
run("var m = mk(); var f = function (e) { LOG.push('f:' + S(e.data)) }; m.port2.addEventListener('message', f); m.port2.start(); m.port2.removeEventListener('message', f); m.port1.postMessage(1)");
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('once:' + S(e.data)) }, { once: true }); m.port2.start(); m.port1.postMessage(1); m.port1.postMessage(2)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('x') }; m.port2.onmessage = null; m.port1.postMessage(1); LOG.push('end')");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('first') }; m.port2.onmessage = function (e) { LOG.push('second') }; m.port1.postMessage(1)");
run("var m = mk(); m.port2.onmessageerror = function (e) { LOG.push('err') }; m.port2.start(); m.port2.addEventListener('message', function (e) { LOG.push('msg:' + S(e.data)) }); m.port1.postMessage(1)");
run("var m = mk(); m.port2.onmessage = {}; m.port1.postMessage(1); LOG.push(typeof m.port2.onmessage)");
run("var m = mk(); m.port2.start(); m.port2.start(); m.port2.addEventListener('message', function (e) { LOG.push('l:' + S(e.data)) }); m.port1.postMessage(1)");

// O MessageEvent entregue.
const ev = (props) => run(`var m = mk(); m.port2.onmessage = function (e) { LOG.push(S(${props})) }; m.port1.postMessage({ k: 1 })`);
ev("e.type");
ev("[e instanceof MessageEvent, e instanceof Event, e.constructor === MessageEvent]");
ev("[e.origin, e.lastEventId, e.source, e.ports, e.ports.length, Object.isFrozen(e.ports), Array.isArray(e.ports)]");
ev("[e.target === m.port2, e.currentTarget === m.port2, e.srcElement === m.port2, e.eventPhase, e.bubbles, e.cancelable, e.composed, e.isTrusted, e.defaultPrevented]");
ev("e.data");
ev("this === m.port2");
ev("e.ports === e.ports");

// Clone estruturado.
run("var m = mk(); var o = { n: 1 }; m.port2.onmessage = function (e) { LOG.push(S(e.data === o) + S(e.data)) }; m.port1.postMessage(o); o.n = 2");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(Object.prototype.toString.call(e.data) + ':' + e.data.get('k')) }; m.port1.postMessage(new Map([['k', 'v']]))");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(S(e.data.self === e.data)) }; var o = {}; o.self = o; m.port1.postMessage(o)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(S(e.data)) }; m.port1.postMessage(undefined); m.port1.postMessage(null); m.port1.postMessage('')");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('got') }; try { m.port1.postMessage(function () {}) } catch (e) { LOG.push(E(e)) }");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('got:' + S(e.data)) }; try { m.port1.postMessage({ get x() { throw new Error('getter') } }) } catch (e) { LOG.push(E(e)) } m.port1.postMessage(2)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(S(e.data)) }; m.port1.postMessage(new Date(86400000)); m.port1.postMessage(new Uint8Array([1, 2, 3]))");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(S([e.data.byteLength, Array.from(new Uint8Array(e.data))])) }; var b = new ArrayBuffer(3); new Uint8Array(b).set([7, 8, 9]); m.port1.postMessage(b, [b]); LOG.push('sender:' + b.byteLength)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(S([e.data.byteLength, Array.from(new Uint8Array(e.data))])) }; var b = new ArrayBuffer(3); new Uint8Array(b).set([7, 8, 9]); m.port1.postMessage(b); LOG.push('sender:' + b.byteLength)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push(S(e.data)) }; m.port1.postMessage(1, { transfer: [] }); m.port1.postMessage(2, [])");

// close.
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); m.port2.close()");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); m.port1.close()");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); m.port1.postMessage(2); m.port1.close(); LOG.push('closed')");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)); m.port2.close() }; m.port1.postMessage(1); m.port1.postMessage(2); m.port1.postMessage(3)");
run("var m = mk(); m.port1.close(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; LOG.push(S(m.port2.postMessage(1)))");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.close(); LOG.push(S(m.port1.postMessage(1))); m.port2.postMessage(2)");
run("var m = mk(); m.port1.onmessage = function (e) { LOG.push('a:' + S(e.data)) }; m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)); m.port1.close() }; m.port1.postMessage(1); m.port2.postMessage(2)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); setTimeout(function () { m.port2.close(); m.port1.postMessage(2) }, 5)");
run("var m = mk(); m.port2.addEventListener('message', function (e) { LOG.push('l:' + S(e.data)) }); m.port1.postMessage(1); m.port2.close(); m.port2.start()");

// Evento `close`: a porta fechada recebe o dela primeiro, a porta par (se ainda aberta) depois das mensagens da volta,
// ambos antes de qualquer timer; `onclose` não existe (propriedade comum); fechar de novo não repete o evento.
run("var m = mk(); m.port1.addEventListener('close', function (e) { LOG.push('c1:' + S([e.constructor.name === 'Event', e.type, e.target === m.port1, e.isTrusted, e.bubbles, e.cancelable])) }); m.port2.addEventListener('close', function (e) { LOG.push('c2:' + e.type) }); m.port1.close(); LOG.push('sync')");
run("var m = mk(); m.port1.addEventListener('close', function () { LOG.push('c1') }); m.port2.addEventListener('close', function () { LOG.push('c2') }); m.port1.close(); m.port2.close()");
run("var m = mk(); m.port1.addEventListener('close', function () { LOG.push('c1') }); m.port1.close(); m.port1.close()");
run("var m = mk(); m.port1.addEventListener('close', function () { LOG.push('c1') }); m.port2.addEventListener('close', function () { LOG.push('c2') }); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; m.port1.postMessage(1); m.port1.close(); Promise.resolve().then(function () { LOG.push('micro') }); setTimeout(function () { LOG.push('t0') }, 0)");
run("var m = mk(); m.port2.addEventListener('close', function () { LOG.push('c2') }); m.port2.close(); m.port1.addEventListener('close', function () { LOG.push('c1') })");
run("var m = mk(); m.port2.onclose = function () { LOG.push('onclose') }; LOG.push(typeof m.port2.onclose); m.port1.close()");
run("var m = mk(); m.port2.addEventListener('close', function () { LOG.push('c2') }); m.port1.close(); setTimeout(function () { m.port2.close() }, 5)");
run("var m = mk(), n = mk(); m.port2.addEventListener('close', function () { LOG.push('m2') }); n.port1.addEventListener('close', function () { LOG.push('n1') }); n.port2.addEventListener('close', function () { LOG.push('n2') }); n.port2.onmessage = function (e) { LOG.push('msg' + e.data) }; n.port1.postMessage(1); n.port1.close(); m.port1.close(); Promise.resolve().then(function () { LOG.push('micro') }); setTimeout(function () { LOG.push('t0') }, 0)");
run("var m = mk(); m.port2.addEventListener('close', function () { LOG.push('c2') }); m.port1.close(); m.port2.start(); LOG.push(S(m.port2.postMessage(1)))");

// Porta transferida por postMessage de outra porta.
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); var p = e.ports[0]; LOG.push(S([e.data, e.ports.length, p instanceof MessagePort, p !== n.port2, e.ports === e.ports])); p.onmessage = function (e2) { LOG.push('t:' + S(e2.data)) } }; m.port1.postMessage('x', [n.port2]); n.port1.postMessage('viaT')");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push('m:' + S(e.data)) }; n.port2.onmessage = function (e) { LOG.push('n2:' + S(e.data)) }; m.port1.postMessage('x', [n.port2]); n.port1.postMessage('after-transfer')");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); e.ports[0].onmessage = function (e2) { LOG.push('t:' + S(e2.data)); e.ports[0].postMessage('back') } }; n.port1.onmessage = function (e) { LOG.push('n1:' + S(e.data)) }; n.port1.postMessage('first'); m.port1.postMessage('x', [n.port2])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push('ports:' + e.ports.length) }; m.port1.postMessage('x', { transfer: [n.port1, n.port2] })");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); e.ports[0].onmessage = function (e2) { LOG.push('t:' + S(e2.data)) } }; n.port1.postMessage('queued-before-transfer'); m.port1.postMessage('x', [n.port2])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push('a'); e.ports[0].start(); e.ports[0].addEventListener('message', function (e2) { LOG.push('t:' + S(e2.data)) }) }; m.port1.postMessage('x', [n.port2]); n.port1.postMessage('hi')");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push('ports:' + e.ports.length + ':' + S(e.data)) }; m.port1.postMessage(n.port2, [n.port2])");
run("var m = mk(); m.port2.start(); m.port1.postMessage(1); setTimeout(function () { m.port2.addEventListener('message', function (e) { LOG.push('late:' + S(e.data)) }) }, 5)");
run("var m = mk(); m.port2.onmessage = function (e) { LOG.push('b:' + S(e.data)) }; try { m.port1.postMessage(m.port2) } catch (e) { LOG.push(E(e)) }");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push('hasRef:' + e.ports[0].hasRef()) }; n.port2.unref(); m.port1.postMessage(1, [n.port2])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push('x') }; m.port1.postMessage(1, [n.port2]); LOG.push(S([n.port2.hasRef()])); try { n.port2.onmessage = function () {}; LOG.push('set-ok') } catch (e) { LOG.push(E(e)) }");

// Porta dentro do dado clonado (MessagePortReferenceTag): serializada pelo índice na lista de transferência e
// reconstruída no destino como uma só porta nova, em qualquer profundidade e em qualquer contêiner.
const nested = (data, check) =>
  run(`var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); var d = e.data; LOG.push(S([e.ports.length, ${check}, e.ports[0] instanceof MessagePort, e.ports[0] !== n.port1])) }; m.port1.postMessage(${data}, [n.port1])`);
nested("{ p: n.port1 }", "d.p === e.ports[0]");
nested("[n.port1]", "Array.isArray(d) && d[0] === e.ports[0]");
nested("[n.port1, n.port1]", "d[0] === d[1] && d[0] === e.ports[0]");
nested("{ a: n.port1, b: n.port1 }", "d.a === d.b && d.a === e.ports[0]");
nested("new Map([['k', n.port1]])", "d.get('k') === e.ports[0]");
nested("new Map([[n.port1, 'v']])", "d.keys().next().value === e.ports[0]");
nested("new Set([n.port1])", "d.has(e.ports[0])");
nested("{ a: { b: { c: [{ d: n.port1 }] } } }", "d.a.b.c[0].d === e.ports[0]");
nested("{ p: n.port1, q: [n.port1], r: new Map([['k', n.port1]]), s: new Set([n.port1]) }", "d.p === d.q[0] && d.r.get('k') === d.p && d.s.has(d.p) && d.p === e.ports[0]");
nested("[n.port1, n.port1]", "e.ports.length");
nested("{ p: n.port1, x: 1 }", "Object.keys(d)");
run("var m = mk(), a = mk(), b = mk(), c = mk(); m.port2.onmessage = function (e) { keep(e); var d = e.data; LOG.push(S([e.ports.length, d.x === e.ports[1], d.y === e.ports[0], d.z === e.ports[2]])) }; m.port1.postMessage({ x: b.port1, y: a.port1, z: c.port1 }, [a.port1, b.port1, c.port1])");
run("var m = mk(), a = mk(), b = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push(S([e.ports.length, e.data[0] === e.ports[1], e.data[1] === e.ports[0]])) }; m.port1.postMessage([b.port2, a.port2], [a.port2, b.port2])");
run("var m = mk(), n = mk(), o = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push(S([e.ports.length, e.data.p === e.ports[0]])) }; m.port1.postMessage({ p: n.port1 }, [n.port1, o.port1])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push(S([e.ports.length, e.data.p === undefined])) }; m.port1.postMessage({ p: 1 }, [n.port1])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); var p = e.data.p; p.onmessage = function (e2) { LOG.push('t:' + S(e2.data)) } }; n.port1.postMessage('queued'); m.port1.postMessage({ p: n.port2 }, [n.port2]); n.port1.postMessage('after')");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); var p = e.data.p; p.onmessage = function (e2) { LOG.push('t:' + S(e2.data)); p.postMessage('back') } }; n.port1.onmessage = function (e) { LOG.push('n1:' + S(e.data)) }; m.port1.postMessage({ p: n.port2 }, [n.port2]); n.port1.postMessage('go')");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push(S([e.ports.length, e.data.p === e.ports[0], e.data.q === e.ports[1]])) }; m.port1.postMessage({ p: n.port1, q: n.port2 }, [n.port1, n.port2])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push(S([e.ports.length, e.data.p === e.ports[0]])) }; m.port1.postMessage({ p: m.port2 }, [m.port2])");
run("var m = mk(), n = mk(); m.port2.onmessage = function (e) { keep(e); LOG.push(S(Object.prototype.toString.call(e.data.p))) }; m.port1.postMessage({ p: n.port1 }, { transfer: [n.port1] })");
run("var m = mk(), n = mk(), b = new ArrayBuffer(4); m.port2.onmessage = function (e) { keep(e); LOG.push(S([e.ports.length, e.data.p === e.ports[0], e.data.b.byteLength, b.byteLength])) }; m.port1.postMessage({ p: n.port1, b: b }, [n.port1, b])");

// Erros com porta no dado ou na lista de transferência (DataCloneError, mensagens medidas no bun 1.4.2).
expr(`(function(m){ return m.port1.postMessage({ p: m.port2 }) })(mk())`);
expr(`(function(m, n){ return m.port1.postMessage({ p: n.port1 }) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage([n.port1]) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage(new Map([['k', n.port1]])) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage(new Set([n.port1])) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage({ a: { b: { c: n.port1 } } }) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage([n.port1], []) })(mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage({ p: n.port1 }, { transfer: [] }) })(mk(), mk())`);
expr(`(function(m, n, o){ return m.port1.postMessage({ p: n.port1, q: o.port1 }, [n.port1]) })(mk(), mk(), mk())`);
expr(`(function(m, n){ return m.port1.postMessage({ p: n.port1 }, [n.port1, n.port1]) })(mk(), mk())`);
expr(`(function(m){ return m.port1.postMessage({ p: m.port1 }, [m.port1]) })(mk())`);
expr(`(function(m){ return m.port1.postMessage({ p: m.port2 }, [m.port2]) })(mk())`);
expr(`(function(m, n){ n.port1.close(); return m.port1.postMessage(1, [n.port1]) })(mk(), mk())`);
expr(`(function(m, n){ n.port1.close(); return m.port1.postMessage({ p: n.port1 }, [n.port1]) })(mk(), mk())`);
expr(`(function(m, n){ m.port1.postMessage(1, [n.port1]); return m.port1.postMessage({ p: n.port1 }, [n.port1]) })(mk(), mk())`);
expr(`(function(m, n){ m.port1.postMessage(1, [n.port1]); return m.port1.postMessage({ p: n.port1 }) })(mk(), mk())`);
expr(`(function(m, n){ m.port1.close(); return m.port1.postMessage({ p: n.port1 }) })(mk(), mk())`);
expr(`(function(m, n){ m.port1.close(); return m.port1.postMessage({ p: n.port1 }, [n.port1]) })(mk(), mk())`);
expr(`(function(m, n){ try { m.port1.postMessage({ p: n.port1 }) } catch (e) {} return [n.port1.postMessage(1), n.port1.hasRef()] })(mk(), mk())`);
expr(`(function(m, n){ var o = { get x() { throw new Error('g') }, p: n.port1 }; try { m.port1.postMessage(o, [n.port1]) } catch (e) {} return n.port1.postMessage(1) })(mk(), mk())`);

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mc-"));
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
