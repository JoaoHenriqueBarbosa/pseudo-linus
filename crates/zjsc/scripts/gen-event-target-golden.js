// Gera tests/golden/event_target_bun.tsv: `EventTarget` e `Event` do global medidos no bun 1.4.2 (descritor, `length`,
// chaves e atributos do construtor e dos protótipos, constantes de fase, construção, erros de `this` e de argumento,
// lista de ouvintes: ordem, duplicata, `once`, `handleEvent`, remoção e inclusão durante o despacho, exceção de
// ouvinte e `handleEvent` não chamável relatados ao `uncaughtException`, tipo com surrogate solto, `stopImmediatePropagation`, retorno de `dispatchEvent`). Colunas: a fonte do programa (JSON) e o valor
// da variável global `R` (JSON). Inclui `AbortController`, `AbortSignal` (com `timeout`, cujos casos rodam o laço de eventos; os casos de processo rodam cada um num bun próprio, para medir o que acontece até o processo sair) e a opção `signal`. Fora do golden de propósito: captura/propagação, aviso de stderr, a posição da
// chave no global, `timeStamp` (só o tipo) e `originalLine`/`sourceURL` dos erros com `code`.
// Uso: bun scripts/gen-event-target-golden.js > tests/golden/event_target_bun.tsv
const { emitRow } = require("./golden-prelude.js");
const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const { runMain } = require("./uncaught-run.js");

// O relato de erro não tratado (`Bun__reportUnhandledError`) chega aqui na hora em que acontece; `U` é zerado por programa.
process.on("uncaughtException", (e) => {
  globalThis.U.push(e instanceof Error ? e.name + ": " + e.message : "value: " + String(e));
});

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.constructor.name + '|' + e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n" +
  "var D = function (d) { return d === undefined ? 'undefined' : [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get, typeof d.set] };\n" +
  "var F = function (f) { return typeof f === 'function' ? [f.name, f.length, f.toString()] : f === undefined ? 'undefined' : typeof f };\n" +
  "var V = function (v) { return typeof v === 'function' ? F(v) : typeof v === 'object' && v !== null ? 'object' : typeof v === 'symbol' ? String(v) : v };\n" +
  "var X = function (o) { return Reflect.ownKeys(o).map(function (k) { var d = Object.getOwnPropertyDescriptor(o, k); return [String(k), 'value' in d ? V(d.value) : 'accessor', d.writable, d.enumerable, d.configurable, F(d.get), F(d.set)] }) };\n" +
  "var et = new EventTarget(), ev = new Event('x'), ep = Event.prototype, tp = EventTarget.prototype, log = [];\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const body = (code) => expr(`(function(){ ${code} })()`);

// Descritor e forma dos construtores.
for (const C of ["EventTarget", "Event"]) {
  expr(`D(Object.getOwnPropertyDescriptor(globalThis, '${C}'))`);
  expr(`${C}.length`);
  expr(`${C}.name`);
  expr(`${C}.toString()`);
  expr(`typeof ${C}`);
  expr(`Object.getPrototypeOf(${C}) === Function.prototype`);
  expr(`D(Object.getOwnPropertyDescriptor(${C}, 'prototype'))`);
  expr(`Object.getPrototypeOf(${C}.prototype) === Object.prototype`);
  expr(`Object.prototype.toString.call(${C}.prototype)`);
  expr(`Object.keys(${C}.prototype)`);
  expr(`Reflect.ownKeys(${C}.prototype).map(String)`);
  expr(`${C}.prototype.constructor === ${C}`);
  expr(`D(Object.getOwnPropertyDescriptor(${C}.prototype, 'constructor'))`);
  expr(`D(Object.getOwnPropertyDescriptor(${C}.prototype, Symbol.toStringTag))`);
  expr(`${C}.prototype[Symbol.toStringTag]`);
}
// Descritores completos (todas as chaves próprias, na ordem do bun).
expr("[X(EventTarget), X(tp), X(Event), X(ep)]");
// Constantes.
expr("[Event.NONE, Event.CAPTURING_PHASE, Event.AT_TARGET, Event.BUBBLING_PHASE, ev.NONE, ev.BUBBLING_PHASE]");
body("'use strict'; Event.NONE = 9; return Event.NONE");
body("'use strict'; ep.AT_TARGET = 9; return ep.AT_TARGET");

// Construção.
expr("Reflect.ownKeys(et)");
expr("Reflect.ownKeys(ev)");
expr("D(Object.getOwnPropertyDescriptor(ev, 'isTrusted'))");
expr("F(Object.getOwnPropertyDescriptor(ev, 'isTrusted').get)");
expr("ev.isTrusted");
expr("Object.keys(ev)");
expr("String(et)");
expr("String(ev)");
expr("et instanceof EventTarget");
expr("ev instanceof Event");
expr("ev instanceof EventTarget");
expr("Object.getPrototypeOf(ev) === ep");
expr("EventTarget()");
expr("Event()");
expr("Event.call({}, 'a')");
expr("new Event()");
expr("new Event(Symbol())");
expr("new Event('a', 5)");
expr("new Event('a', 'str')");
expr("[new Event(undefined).type, new Event(null).type, new Event(12).type, new Event({ toString() { return 'ts' } }).type]");
expr("new Event({ toString() { throw new RangeError('boom') } })");
expr("new Event('a', null).cancelable");
expr("new Event('a', undefined).bubbles");
expr("(function(){ var o = new Event('a', { bubbles: 1, cancelable: 'x', composed: [] }); return [o.bubbles, o.cancelable, o.composed, o.defaultPrevented] })()");
expr("(function(){ var r = []; new Event('a', { get bubbles() { r.push('b'); return 0 }, get cancelable() { r.push('c'); return 0 }, get composed() { r.push('d'); return 0 } }); return r })()");
expr("(function(){ class X extends Event {} var x = new X('k'); return [x.type, Object.getPrototypeOf(x) === X.prototype, x instanceof Event] })()");
expr("(function(){ class X extends EventTarget {} var x = new X(); var r = []; x.addEventListener('a', function () { r.push(this === x) }); x.dispatchEvent(new Event('a')); return [r, x instanceof EventTarget] })()");
expr("Object.getPrototypeOf(Reflect.construct(Event, ['a'], Object)) === Object.prototype");
expr("Object.getPrototypeOf(Reflect.construct(EventTarget, [], Object)) === Object.prototype");
expr("[ev.type, ev.target, ev.currentTarget, ev.eventPhase, ev.defaultPrevented, ev.bubbles, ev.cancelable, ev.composed, ev.isTrusted, typeof ev.timeStamp, ev.srcElement, ev.returnValue, ev.cancelBubble]");
expr("ev.composedPath()");
expr("Array.isArray(ev.composedPath())");
// Setters de dados (acessores sem setter: sloppy ignora, strict lança).
body("ev.type = 'y'; return ev.type");
body("'use strict'; ev.type = 'y'");
body("ev.cancelBubble = true; return ev.cancelBubble");
body("ev.cancelBubble = false; return ev.cancelBubble");
body("var o = new Event('a', { cancelable: true }); o.returnValue = false; return [o.defaultPrevented, o.returnValue]");
body("var o = new Event('a'); o.returnValue = false; return [o.defaultPrevented, o.returnValue]");
body("var o = new Event('a', { cancelable: true }); o.returnValue = true; return [o.defaultPrevented, o.returnValue]");

// Brand check.
for (const k of ["type", "target", "currentTarget", "eventPhase", "cancelBubble", "bubbles", "cancelable", "defaultPrevented", "composed", "timeStamp", "srcElement", "returnValue"]) {
  expr(`Object.getOwnPropertyDescriptor(ep, '${k}').get.call({})`);
  expr(`Object.getOwnPropertyDescriptor(ep, '${k}').get.call(ep)`);
}
expr("Object.getOwnPropertyDescriptor(ep, 'type').get.call(null)");
expr("Object.getOwnPropertyDescriptor(ep, 'type').get.call(et)");
expr("Object.getOwnPropertyDescriptor(ep, 'cancelBubble').set.call({}, true)");
expr("Object.getOwnPropertyDescriptor(ep, 'returnValue').set.call({}, true)");
for (const k of ["composedPath", "stopPropagation", "stopImmediatePropagation", "preventDefault"]) {
  expr(`ep.${k}.call({})`);
  expr(`ep.${k}.call(et)`);
}
expr("ep.initEvent.call({}, 'a')");
expr("ep.initEvent.call({})");
for (const k of ["addEventListener", "removeEventListener", "dispatchEvent"]) {
  expr(`tp.${k}.call({})`);
  expr(`tp.${k}.call(ev)`);
  expr(`tp.${k}.call(null)`);
  expr(`et.${k}()`);
}
expr("et.addEventListener('a')");
expr("et.removeEventListener('a')");
expr("tp.dispatchEvent.call(tp, new Event('a'))");
expr("tp.addEventListener.call(tp, 'a', function () {})");

// Erros de argumento.
expr("et.dispatchEvent({})");
expr("et.dispatchEvent(5)");
expr("et.dispatchEvent(null)");
expr("et.dispatchEvent({ type: 'a' })");
expr("et.dispatchEvent(Object.create(ep))");
expr("et.addEventListener('a', 5)");
expr("et.addEventListener('a', 'f')");
expr("et.addEventListener('a', true)");
expr("et.removeEventListener('a', 5)");
expr("et.addEventListener('a', {})");
expr("et.addEventListener(Symbol(), function () {})");
expr("et.addEventListener({ toString() { throw new RangeError('boom') } }, function () {})");
expr("et.addEventListener('a', function () {}, 5)");
expr("et.addEventListener('a', function () {}, null)");
expr("et.addEventListener('a', function () {}, { get capture() { throw new RangeError('cap') } })");

// Retornos.
expr("[et.addEventListener('a', function () {}), et.removeEventListener('a', function () {}), et.dispatchEvent(new Event('zz'))]");
expr("et.addEventListener('a', null)");
expr("et.addEventListener('a', undefined)");
expr("et.removeEventListener('a', null)");

// Lista de ouvintes.
body("var f = function () { log.push('f') }, g = { handleEvent: function () { log.push('g:' + (this === g)) } }; et.addEventListener('t', f); et.addEventListener('t', g); et.addEventListener('t', f); et.addEventListener('t', function () { log.push('once') }, { once: true }); et.dispatchEvent(new Event('t')); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f); et.addEventListener('t', f); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f); et.removeEventListener('t', f); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f); et.removeEventListener('u', f); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f, true); et.addEventListener('t', f, false); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f, { capture: true }); et.addEventListener('t', f); et.removeEventListener('t', f); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f, { capture: true }); et.addEventListener('t', f); et.removeEventListener('t', f, true); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f, { capture: true }); et.removeEventListener('t', f, { capture: true }); et.dispatchEvent(new Event('t')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('t', f, { once: true }); et.addEventListener('t', f); et.dispatchEvent(new Event('t')); et.dispatchEvent(new Event('t')); return log");
body("et.addEventListener('t', function (e) { log.push([this === et, e.target === et, e.currentTarget === et, e.eventPhase, e.type, arguments.length]) }); var o = new Event('t'); et.dispatchEvent(o); return [log, o.target === et, o.currentTarget, o.eventPhase]");
body("var o = new Event('t'); et.addEventListener('t', function (e) { log.push(e === o) }); et.dispatchEvent(o); et.dispatchEvent(o); return log");
body("var h = { handleEvent: function (e) { log.push([this === h, arguments.length, e.type]) } }; et.addEventListener('t', h); et.dispatchEvent(new Event('t')); return log");
body("var h = function () { log.push('fn') }; h.handleEvent = function () { log.push('he') }; et.addEventListener('t', h); et.dispatchEvent(new Event('t')); return log");
body("var a = function () { log.push('a'); et.removeEventListener('m', b); et.addEventListener('m', c) }, b = function () { log.push('b') }, c = function () { log.push('c') }; et.addEventListener('m', a); et.addEventListener('m', b); et.dispatchEvent(new Event('m')); et.dispatchEvent(new Event('m')); return log");
body("var a = function () { log.push('a'); et.removeEventListener('m', a) }, b = function () { log.push('b') }; et.addEventListener('m', a); et.addEventListener('m', b); et.dispatchEvent(new Event('m')); et.dispatchEvent(new Event('m')); return log");
body("et.addEventListener('s', function (e) { log.push(1); e.stopImmediatePropagation() }); et.addEventListener('s', function () { log.push(2) }); var o = new Event('s'); et.dispatchEvent(o); return [log, o.cancelBubble]");
body("et.addEventListener('s', function (e) { log.push(1); e.stopPropagation(); log.push(e.cancelBubble) }); et.addEventListener('s', function () { log.push(2) }); var o = new Event('s'); et.dispatchEvent(o); return [log, o.cancelBubble]");
body("et.addEventListener('s', function (e) { e.cancelBubble = true }); et.addEventListener('s', function () { log.push(2) }); et.dispatchEvent(new Event('s')); return log");
body("var o = new Event('s'); o.stopImmediatePropagation(); et.addEventListener('s', function () { log.push(1) }); et.dispatchEvent(o); return log");
body("var o = new Event('s'); o.stopPropagation(); return [o.cancelBubble]");
body("et.addEventListener('c', function (e) { e.preventDefault() }); var a = new Event('c', { cancelable: true }), b = new Event('c'); return [et.dispatchEvent(a), a.defaultPrevented, et.dispatchEvent(b), b.defaultPrevented]");
body("et.addEventListener('c', function (e) { log.push([e.defaultPrevented, e.returnValue]); e.preventDefault(); log.push([e.defaultPrevented, e.returnValue]) }); et.dispatchEvent(new Event('c', { cancelable: true })); return log");
body("et.addEventListener('c', function (e) { e.returnValue = false }); var a = new Event('c', { cancelable: true }); return [et.dispatchEvent(a), a.defaultPrevented]");
body("et.addEventListener('c', function (e) { log.push(e.composedPath().length, e.composedPath()[0] === et) }); et.dispatchEvent(new Event('c')); return log");
body("var e5 = new Event('nest'); et.addEventListener('nest', function () { try { et.dispatchEvent(e5) } catch (e) { log.push(e.constructor.name + '|' + e.name + '|' + e.message + '|' + e.code) } }); et.dispatchEvent(e5); return log");
body("var e5 = new Event('nest'), other = new EventTarget(); et.addEventListener('nest', function () { try { other.dispatchEvent(e5) } catch (e) { log.push(e.code) } }); et.dispatchEvent(e5); return log");
body("var e5 = new Event('nest'); et.addEventListener('nest', function () { log.push(1) }); et.dispatchEvent(e5); et.dispatchEvent(e5); return log");
body("et.addEventListener('t', function () { log.push(et.dispatchEvent(new Event('inner'))) }); et.addEventListener('inner', function () { log.push('inner') }); et.dispatchEvent(new Event('t')); return log");
body("var a = new EventTarget(), b = new EventTarget(); a.addEventListener('t', function () { log.push('a') }); b.addEventListener('t', function () { log.push('b') }); b.dispatchEvent(new Event('t')); return log");
body("var o = new Event('t'); var a = new EventTarget(), b = new EventTarget(); a.dispatchEvent(o); var t1 = o.target === a; b.dispatchEvent(o); return [t1, o.target === b, o.srcElement === b]");
body("var o = new Event('t'); o.initEvent('u', true, true); return [o.type, o.bubbles, o.cancelable]");
// Sem propagação entre alvos: subclasse com getTheParent/parentNode, bubbles e composed não alcançam outro alvo.
body("class P extends EventTarget { get parentNode() { return other } getTheParent() { return other } } var other = new EventTarget(), p = new P(); other.addEventListener('x', function () { log.push('other') }); p.addEventListener('x', function (e) { log.push(['p', e.eventPhase, e.target === p, e.currentTarget === p, e.composedPath().length, e.composedPath()[0] === p]) }); p.dispatchEvent(new Event('x', { bubbles: true, composed: true })); return log");
body("var a = new EventTarget(), b = new EventTarget(); b.addEventListener('x', function () { log.push('b') }); a.addEventListener('x', function (e) { log.push([e.eventPhase, e.composedPath().length]) }, true); a.addEventListener('x', function (e) { log.push([e.eventPhase, e.composedPath().length]) }); a.dispatchEvent(new Event('x', { bubbles: true })); return log");
body("var m = new MessageChannel(), l = []; m.port1.addEventListener('x', function (e) { l.push([e.eventPhase, e.target === m.port1, e.currentTarget === m.port1, e.composedPath().length]) }); m.port1.dispatchEvent(new Event('x', { bubbles: true, composed: true })); m.port1.close(); m.port2.close(); return l");
body("var s = AbortSignal.any([new AbortController().signal]); s.addEventListener('x', function (e) { log.push([e.eventPhase, e.target === s, e.currentTarget === s, e.composedPath().length]) }); s.dispatchEvent(new Event('x', { bubbles: true, composed: true })); return log");
// Estado do Event durante e após o despacho, sem cadeia de pais.
body("var o = new Event('p', { bubbles: true, cancelable: true, composed: true }); et.addEventListener('p', function (e) { log.push([e.eventPhase, e.target === et, e.currentTarget === et, e.srcElement === et, e.cancelBubble, e.defaultPrevented, e.bubbles, e.composed, e.isTrusted, typeof e.timeStamp, e.timeStamp >= 0]) }); et.dispatchEvent(o); log.push([o.eventPhase, o.target === et, o.currentTarget, o.srcElement === et, o.cancelBubble, o.composedPath().length]); return log");
body("var o = new Event('p'); var path; et.addEventListener('p', function (e) { path = e.composedPath(); log.push(path.length, path[0] === et, e.composedPath() === path) }); et.dispatchEvent(o); log.push(path.length, o.composedPath().length); return log");
body("et.addEventListener('p', function (e) { e.stopPropagation(); log.push(1, e.cancelBubble) }); et.addEventListener('p', function (e) { log.push(2, e.cancelBubble) }); var o = new Event('p'); et.dispatchEvent(o); et.dispatchEvent(o); return [log, o.cancelBubble]");
body("et.addEventListener('p', function (e) { e.stopImmediatePropagation(); log.push(1) }); et.addEventListener('p', function () { log.push(2) }); var o = new Event('p'); et.dispatchEvent(o); et.dispatchEvent(o); return [log, o.cancelBubble]");
body("et.addEventListener('p', function (e) { e.cancelBubble = true; e.cancelBubble = false; log.push(e.cancelBubble) }); et.addEventListener('p', function (e) { log.push(e.cancelBubble) }); et.dispatchEvent(new Event('p')); return log");
body("var o = new Event('p'); o.cancelBubble = true; et.addEventListener('p', function (e) { log.push(e.cancelBubble) }); et.dispatchEvent(o); return [log, o.cancelBubble]");
body("var o = new Event('p'); o.stopPropagation(); et.addEventListener('p', function (e) { log.push(e.cancelBubble) }); et.dispatchEvent(o); return [log, o.cancelBubble]");
body("var o = new Event('p'); et.addEventListener('p', function (e) { e.preventDefault(); e.returnValue = true; log.push([e.defaultPrevented, e.returnValue]) }); return [et.dispatchEvent(o), log, o.defaultPrevented, o.returnValue]");
body("var o = new Event('p', { cancelable: true }); o.preventDefault(); et.addEventListener('p', function (e) { log.push(e.defaultPrevented) }); return [et.dispatchEvent(o), log, o.defaultPrevented]");
body("var o = new Event('p', { cancelable: true }); et.addEventListener('p', function (e) { e.preventDefault() }); et.dispatchEvent(o); return [et.dispatchEvent(o), o.defaultPrevented]");
body("var o = new Event('p'); et.addEventListener('p', function (e) { try { et.dispatchEvent(e) } catch (x) { log.push(x.name + '|' + x.code) } log.push(e.eventPhase, e.currentTarget === et) }); et.dispatchEvent(o); log.push(o.eventPhase); et.dispatchEvent(o); return log");
body("var o = new Event('p'); et.addEventListener('p', function (e) { throw new Error('x') }); et.addEventListener('p', function (e) { log.push('after') }); et.dispatchEvent(o); return [log, o.eventPhase, o.currentTarget, o.target === et]");
// `timeStamp` é sempre 0 no bun 1.4.2, antes e durante o despacho, e também no `CustomEvent`.
body("var a = new Event('p'), t = []; et.addEventListener('p', function (e) { t.push(e.timeStamp) }); et.dispatchEvent(a); return [a.timeStamp, t, new CustomEvent('c').timeStamp]");
// Captura antes do restante no próprio alvo, `cancelBubble` volta a false depois do despacho, evento já em despacho.
body("var f = function () { log.push('f') }; et.addEventListener('q', function () { log.push('b') }); et.addEventListener('q', function () { log.push('c1') }, true); et.addEventListener('q', function () { log.push('c2') }, { capture: true }); et.dispatchEvent(new Event('q')); return log");
body("var o = new Event('w'); et.addEventListener('w', function (e) { e.stopPropagation(); log.push(e.cancelBubble) }); et.dispatchEvent(o); var after = o.cancelBubble; o.cancelBubble = false; return [log, after, o.cancelBubble]");
body("var o = new Event('y', { cancelable: true }); et.addEventListener('y', function (e) { try { et.dispatchEvent(e) } catch (x) { log.push(E(x)) } }); et.dispatchEvent(o); return log");
body("et.addEventListener('p', function (e) { e.preventDefault(); log.push(e.defaultPrevented) }, { passive: true }); var o = new Event('p', { cancelable: true }); return [et.dispatchEvent(o), o.defaultPrevented, o.returnValue, log]");
body("var a = new Event('p'), b = new Event('p'); return [a.timeStamp <= b.timeStamp, typeof a.timeStamp, Number.isFinite(a.timeStamp), a.timeStamp === a.timeStamp]");
body("var o = new Event('p'); et.addEventListener('p', function (e) { e.initEvent('q', true, true); log.push([e.type, e.bubbles, e.cancelable]) }); et.dispatchEvent(o); return [log, o.type]");
body("var o = new Event('p'); et.addEventListener('p', function (e) { log.push(e.currentTarget === et) }); var t2 = new EventTarget(); t2.addEventListener('p', function (e) { log.push(e.currentTarget === t2, e.target === t2) }); et.dispatchEvent(o); t2.dispatchEvent(o); return [log, o.currentTarget]");
body("var o = new Event('t', { bubbles: true }); o.initEvent('u'); return [o.type, o.bubbles, o.cancelable]");
body("var o = new Event('t'); return o.initEvent()");
body("et.addEventListener('t', function (e) { e.initEvent('zz', true, true); log.push(e.type, e.bubbles) }); et.dispatchEvent(new Event('t')); return log");
body("var o = new Event('t', { cancelable: true }); o.preventDefault(); o.initEvent('t', false, true); return o.defaultPrevented");
body("et.addEventListener('', function () { log.push('empty') }); et.dispatchEvent(new Event('')); return log");
body("et.addEventListener(1, function () { log.push('num') }); et.dispatchEvent(new Event('1')); return log");
body("et.addEventListener(undefined, function () { log.push('undef') }); et.dispatchEvent(new Event('undefined')); return log");
body("et.addEventListener('T', function () { log.push('upper') }); et.dispatchEvent(new Event('t')); return log");
body("var n = 0; et.addEventListener('t', function () { n++ }); et.addEventListener('t', function () { n++ }); et.dispatchEvent(new Event('t')); return n");
body("et.addEventListener('t', function () { log.push(1) }); et.addEventListener('t', function () { log.push(2) }); et.dispatchEvent(new Event('t')); return log");
// Opções de addEventListener: capture, once, passive, signal, deduplicação, ordem e reentrada.
body("et.addEventListener('a', function (e) { try { e.preventDefault() } catch (x) {} log.push(e.defaultPrevented) }, { passive: true }); var o = new Event('a', { cancelable: true }); return [et.dispatchEvent(o), o.defaultPrevented, log]");
body("et.addEventListener('a', function (e) { e.preventDefault(); log.push(e.defaultPrevented) }, { passive: false }); var o = new Event('a', { cancelable: true }); return [et.dispatchEvent(o), o.defaultPrevented, log]");
body("et.addEventListener('a', function (e) { e.returnValue = false }, { passive: true }); var o = new Event('a', { cancelable: true }); et.dispatchEvent(o); return [o.defaultPrevented, o.returnValue]");
body("et.addEventListener('a', function (e) { e.preventDefault() }, { passive: true }); et.addEventListener('a', function (e) { e.preventDefault(); log.push(e.defaultPrevented) }); var o = new Event('a', { cancelable: true }); et.dispatchEvent(o); return [log, o.defaultPrevented]");
body("et.addEventListener('a', function () { throw 1 }, { passive: true }); et.addEventListener('a', function (e) { e.preventDefault() }); var o = new Event('a', { cancelable: true }); et.dispatchEvent(o); return [o.defaultPrevented, U.length]");
body("var h = { handleEvent: function (e) { log.push(this === h); e.preventDefault() } }; et.addEventListener('a', h, { once: true, passive: true }); var o = new Event('a', { cancelable: true }); et.dispatchEvent(o); return [log, o.defaultPrevented]");
body("var a = new AbortController(), f = function () { log.push(1) }; et.addEventListener('a', f, { signal: a.signal }); et.dispatchEvent(new Event('a')); a.abort(); et.dispatchEvent(new Event('a')); return log");
body("var a = new AbortController(); et.addEventListener('a', function () { log.push(1); a.abort() }, { signal: a.signal }); et.addEventListener('a', function () { log.push(2) }, { signal: a.signal }); et.dispatchEvent(new Event('a')); et.dispatchEvent(new Event('a')); return log");
body("var a = new AbortController(), b = new AbortController(), f = function () { log.push(1) }; et.addEventListener('a', f, { signal: a.signal }); et.addEventListener('a', f, { signal: b.signal }); a.abort(); et.dispatchEvent(new Event('a')); return log");
body("var a = new AbortController(), f = function () { log.push(1) }; et.addEventListener('a', f, { once: true, signal: a.signal }); et.dispatchEvent(new Event('a')); a.abort(); et.dispatchEvent(new Event('a')); return log");
body("var a = new AbortController(), f = function () { log.push(1) }; et.addEventListener('a', f, { signal: a.signal }); a.abort(); et.addEventListener('a', f); et.dispatchEvent(new Event('a')); a.abort(); return log");
body("var a = new AbortController(); a.abort(); et.addEventListener('a', function () { log.push(1) }, { signal: a.signal }); et.dispatchEvent(new Event('a')); return log");
expr("et.addEventListener('a', function () {}, { signal: null })");
expr("et.addEventListener('a', function () {}, { signal: {} })");
expr("et.addEventListener('a', function () {}, { signal: 5 })");
body("et.addEventListener('a', function () {}, { signal: undefined }); return 1");
body("var r = []; var o = { get capture() { r.push('c'); return 0 }, get once() { r.push('o'); return 0 }, get passive() { r.push('p'); return 0 }, get signal() { r.push('s'); return undefined } }; et.addEventListener('a', function () {}, o); et.removeEventListener('a', function () {}, o); return r");
body("var f = function () { log.push('f') }; et.addEventListener('a', f, { capture: 1 }); et.removeEventListener('a', f, { capture: 'x' }); et.dispatchEvent(new Event('a')); et.addEventListener('a', f, 'yes'); et.addEventListener('a', f, { capture: true }); et.removeEventListener('a', f, []); et.dispatchEvent(new Event('a')); return log");
body("et.addEventListener('a', function () { log.push('n1') }); et.addEventListener('a', function () { log.push('c1') }, { capture: true }); et.addEventListener('a', function () { log.push('c2') }, true); et.addEventListener('a', function () { log.push('n2') }); et.dispatchEvent(new Event('a')); return log");
body("et.addEventListener('a', function (e) { log.push(e.eventPhase) }, true); et.addEventListener('a', function (e) { log.push(e.eventPhase) }); et.dispatchEvent(new Event('a')); return log");
body("et.onfoo = function () { log.push('attr') }; et.addEventListener('foo', function () { log.push('cap') }, true); et.addEventListener('foo', function () { log.push('bub') }); et.dispatchEvent(new Event('foo')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('a', f, { capture: true, once: true }); et.addEventListener('a', f); et.dispatchEvent(new Event('a')); et.dispatchEvent(new Event('a')); return log");
body("var f = function () { log.push('f') }; et.addEventListener('a', function () { log.push('c'); et.removeEventListener('a', f) }, true); et.addEventListener('a', f); et.dispatchEvent(new Event('a')); return log");
body("et.addEventListener('a', function (e) { log.push('c'); e.stopPropagation() }, true); et.addEventListener('a', function () { log.push('n') }); et.dispatchEvent(new Event('a')); return log");
body("var h = { handleEvent: function () { log.push(1) } }; et.addEventListener('a', h); et.addEventListener('a', h); et.addEventListener('a', h, true); et.removeEventListener('a', h); et.dispatchEvent(new Event('a')); return log");
body("et.addEventListener('a', function () { log.push(1); et.addEventListener('a', function () { log.push('new') }) }); et.dispatchEvent(new Event('a')); var n = log.length; et.dispatchEvent(new Event('a')); return [n, log]");
body("et.addEventListener('a', function () { log.push(1); et.dispatchEvent(new Event('a')) }, { once: true }); et.dispatchEvent(new Event('a')); return log");
body("var b = function () { log.push('b') }; et.addEventListener('a', function () { et.removeEventListener('a', b); et.addEventListener('a', b) }); et.addEventListener('a', b); et.dispatchEvent(new Event('a')); return log");
body("var b = function () { log.push('b') }; et.addEventListener('a', function () { log.push('a'); et.removeEventListener('a', b); et.addEventListener('a', b) }); et.addEventListener('a', b); et.dispatchEvent(new Event('a')); et.dispatchEvent(new Event('a')); return log");
body("var f = function () { log.push(1) }; et.addEventListener('a', f, { once: 1 }); et.dispatchEvent(new Event('a')); et.dispatchEvent(new Event('a')); return log");
// O toString das funções.
expr("[tp.addEventListener.toString(), ep.initEvent.toString(), Object.getOwnPropertyDescriptor(ep, 'type').get.toString()]");
expr("[typeof tp.addEventListener.prototype, typeof ep.preventDefault.prototype]");

// Exceção de ouvinte e `handleEvent` não chamável: o bun relata cada uma ao `uncaughtException` na hora, na ordem em que
// acontecem, e o despacho segue (sem handler, o relato imprime o erro em stderr e o processo sai com código 1 no fim do
// script; o handler do gerador o impede). O relato vira o vetor global `U` (texto `nome: mensagem`, ou `value: ...` para
// o que não é `Error`); o porte o alimenta pelo gancho de erro não tratado do `VM`, no mesmo formato.
body("et.addEventListener('t', function () { log.push('a'); throw new RangeError('boom') }); et.addEventListener('t', function () { log.push('b') }); var r = et.dispatchEvent(new Event('t')); log.push('end'); return [r, log, U]");
body("et.addEventListener('t', function () { log.push('a'); throw 5 }); et.addEventListener('t', function () { log.push('b'); throw { message: 'm', name: 'n' } }); et.addEventListener('t', function () { throw null }); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', function () { log.push('a'); throw new TypeError('once') }, { once: true }); et.dispatchEvent(new Event('t')); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', function (e) { e.preventDefault(); throw new Error('x') }); var o = new Event('t', { cancelable: true }); return [et.dispatchEvent(o), o.defaultPrevented, U]");
body("et.addEventListener('t', function (e) { throw new Error('first') }); et.addEventListener('t', function (e) { e.stopImmediatePropagation(); throw new Error('second') }); et.addEventListener('t', function () { log.push('never') }); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', { handleEvent: 5 }); et.addEventListener('t', function () { log.push('after') }); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', {}); et.addEventListener('t', function () { log.push('after') }); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', { handleEvent: null }); et.addEventListener('t', { handleEvent: 'str' }); et.addEventListener('t', { handleEvent: {} }); et.addEventListener('t', function () { log.push('after') }); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', { handleEvent: function () { throw new RangeError('he') } }); et.addEventListener('t', function () { log.push('after') }); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', { get handleEvent() { log.push('get'); throw new RangeError('g') } }); et.addEventListener('t', function () { log.push('after') }); var r = et.dispatchEvent(new Event('t')); return [r, log, U]");
body("et.addEventListener('t', { get handleEvent() { log.push('get'); return function () { log.push('called') } } }); et.dispatchEvent(new Event('t')); return [log, U]");
body("var h = { get handleEvent() { log.push('get'); return 5 } }; et.addEventListener('t', h); et.dispatchEvent(new Event('t')); return [log, U]");
body("var f = function () {}; f.handleEvent = 5; et.addEventListener('t', f); et.dispatchEvent(new Event('t')); return [log, U]");
body("et.addEventListener('t', function () { throw new Error('in-outer'); }); et.addEventListener('t', function () { et.dispatchEvent(new Event('inner')) }); et.addEventListener('inner', function () { throw new Error('in-inner') }); et.dispatchEvent(new Event('t')); return U");
// Tipo como string JS (UTF-16): surrogate solto é um tipo, distinto de U+FFFD.
body("var k = 'a\\ud800b'; et.addEventListener(k, function () { log.push('lone') }); et.addEventListener('a\\ufffdb', function () { log.push('fffd') }); et.dispatchEvent(new Event(k)); et.dispatchEvent(new Event('a\\ufffdb')); return [log, new Event(k).type === k, new Event(k).type.length, new Event(k).type.charCodeAt(1)]");
body("var o = new Event('x'); o.initEvent('p\\ud83d', true, false); return [o.type.length, o.type.charCodeAt(1), o.type === 'p\\ud83d']");
body("var k = '\\ud83d\\ude00'; et.addEventListener(k, function () { log.push('pair') }); et.dispatchEvent(new Event('\\ud83d')); et.dispatchEvent(new Event(k)); return [log, new Event(k).type === k]");
body("var k = 'z\\udc00'; var o = new Event(k); et.addEventListener(k, function (e) { log.push(e.type === k, e.type.length) }); et.addEventListener(k, function () { try { et.dispatchEvent(o) } catch (e) { log.push(e.message.length > 0, e.code) } }); et.dispatchEvent(o); return log");
// A mensagem de ERR_EVENT_RECURSION carrega o tipo em UTF-16 (surrogate solto intacto).
body("var k = 'z\\udc00'; var o = new Event(k); et.addEventListener(k, function () { try { et.dispatchEvent(o) } catch (e) { log.push(e.message.length, Array.from(e.message, function (c) { return c.charCodeAt(0) }).join(','), e.message.charCodeAt(12), e.code) } }); et.dispatchEvent(o); return log");
body("var k = 'p\\ud83d\\ude00q'; var o = new Event(k); et.addEventListener(k, function () { try { et.dispatchEvent(o) } catch (e) { log.push(e.message, e.message.length) } }); et.dispatchEvent(o); return log");

// CustomEvent: forma, detail, initCustomEvent, brand check e erros.
expr("[X(CustomEvent), X(CustomEvent.prototype)]");
expr("D(Object.getOwnPropertyDescriptor(globalThis, 'CustomEvent'))");
expr("[CustomEvent.length, CustomEvent.name, CustomEvent.toString(), typeof CustomEvent]");
expr("[Object.getPrototypeOf(CustomEvent) === Event, Object.getPrototypeOf(CustomEvent.prototype) === ep, CustomEvent.prototype.constructor === CustomEvent]");
expr("D(Object.getOwnPropertyDescriptor(CustomEvent, 'prototype'))");
expr("[Object.keys(CustomEvent.prototype), Reflect.ownKeys(CustomEvent.prototype).map(String), Object.keys(CustomEvent)]");
expr("[D(Object.getOwnPropertyDescriptor(CustomEvent.prototype, 'detail')), D(Object.getOwnPropertyDescriptor(CustomEvent.prototype, 'initCustomEvent')), D(Object.getOwnPropertyDescriptor(CustomEvent.prototype, Symbol.toStringTag))]");
expr("[F(CustomEvent.prototype.initCustomEvent), F(Object.getOwnPropertyDescriptor(CustomEvent.prototype, 'detail').get), typeof CustomEvent.prototype.initCustomEvent.prototype]");
expr("[Object.prototype.toString.call(CustomEvent.prototype), Object.prototype.toString.call(new CustomEvent('a')), String(new CustomEvent('a'))]");
expr("[CustomEvent.NONE, CustomEvent.CAPTURING_PHASE, CustomEvent.AT_TARGET, CustomEvent.BUBBLING_PHASE, new CustomEvent('a').BUBBLING_PHASE, Object.getOwnPropertyDescriptor(CustomEvent, 'NONE')]");
expr("(function(){ var c = new CustomEvent('a'); return [Reflect.ownKeys(c).map(String), Object.keys(c), c.detail, c.type, c.isTrusted, c instanceof CustomEvent, c instanceof Event, Object.getPrototypeOf(c) === CustomEvent.prototype] })()");
expr("(function(){ var c = new CustomEvent('a'); return [c.type, c.target, c.currentTarget, c.eventPhase, c.defaultPrevented, c.bubbles, c.cancelable, c.composed, typeof c.timeStamp, c.srcElement, c.returnValue, c.cancelBubble, c.composedPath()] })()");
expr("CustomEvent('a')");
expr("CustomEvent.call({}, 'a')");
expr("new CustomEvent()");
expr("new CustomEvent(Symbol())");
expr("new CustomEvent('a', 5)");
expr("new CustomEvent('a', 'str')");
expr("new CustomEvent('a', true)");
expr("new CustomEvent({ toString() { throw new RangeError('boom') } })");
expr("[new CustomEvent(undefined).type, new CustomEvent(null).type, new CustomEvent(12).type]");
expr("[new CustomEvent('a', null).detail, new CustomEvent('a', undefined).detail, new CustomEvent('a', {}).detail, new CustomEvent('a', { detail: undefined }).detail, new CustomEvent('a', { detail: null }).detail]");
expr("[new CustomEvent('a', { detail: 5 }).detail, new CustomEvent('a', { detail: 's' }).detail, new CustomEvent('a', { detail: 0 }).detail, new CustomEvent('a', { detail: false }).detail, new CustomEvent('a', { detail: [1, { a: 2 }] }).detail]");
body("var o = {}; var c = new CustomEvent('a', { detail: o }); return [c.detail === o, c.detail === c.detail]");
body("var c = new CustomEvent('a', { detail: 1, bubbles: 1, cancelable: 'x', composed: [] }); return [c.detail, c.bubbles, c.cancelable, c.composed]");
body("var r = []; new CustomEvent('a', { get detail() { r.push('d'); return 1 }, get bubbles() { r.push('b'); return 1 }, get cancelable() { r.push('c') }, get composed() { r.push('p') } }); return r");
expr("new CustomEvent('a', { get detail() { throw new RangeError('d') } })");
expr("new CustomEvent('a', { get bubbles() { throw new RangeError('b') }, get detail() { throw new RangeError('d') } })");
body("class K extends CustomEvent {} var k = new K('k', { detail: 3 }); return [k.detail, k.type, k instanceof CustomEvent, k instanceof Event, Object.getPrototypeOf(k) === K.prototype]");
expr("Object.getPrototypeOf(Reflect.construct(CustomEvent, ['a'], Object)) === Object.prototype");
// Brand check: Event puro e objeto alheio não passam.
for (const k of ["{}", "ep", "new Event('a')", "null", "CustomEvent.prototype", "et"]) {
  expr(`Object.getOwnPropertyDescriptor(CustomEvent.prototype, 'detail').get.call(${k})`);
  expr(`CustomEvent.prototype.initCustomEvent.call(${k}, 'a')`);
}
expr("CustomEvent.prototype.initCustomEvent.call({})");
expr("CustomEvent.prototype.initCustomEvent.call(new Event('a'))");
expr("CustomEvent.prototype.detail");
expr("Object.getOwnPropertyDescriptor(CustomEvent.prototype, 'detail').get.call(new CustomEvent('a', { detail: 7 }))");
// Os métodos e getters de Event aceitam um CustomEvent.
body("var c = new CustomEvent('a', { cancelable: true }); c.preventDefault(); return [Object.getOwnPropertyDescriptor(ep, 'defaultPrevented').get.call(c), ep.stopPropagation.call(c), ep.composedPath.call(c), Object.getOwnPropertyDescriptor(ep, 'type').get.call(c)]");
body("var c = new CustomEvent('a', { detail: 1, cancelable: true }); c.preventDefault(); return [c.defaultPrevented, c.returnValue, c.cancelBubble]");
// initCustomEvent.
body("var c = new CustomEvent('a', { detail: 1 }); c.initCustomEvent('b', true, true, { z: 1 }); return [c.type, c.bubbles, c.cancelable, c.detail]");
body("var c = new CustomEvent('a', { detail: 1 }); var r = c.initCustomEvent('b'); return [r, c.type, c.bubbles, c.cancelable, c.detail]");
body("var c = new CustomEvent('a', { detail: 1 }); c.initCustomEvent('b', false, false); return c.detail");
body("var c = new CustomEvent('a', { detail: 1 }); c.initCustomEvent('b', false, false, undefined); return c.detail");
body("var c = new CustomEvent('a', { detail: 3 }); c.initCustomEvent('b', false, false, 0); return c.detail");
body("var c = new CustomEvent('a'); c.initCustomEvent('b', 1, 'x', 0); return [c.bubbles, c.cancelable, c.detail]");
body("var c = new CustomEvent('a', { cancelable: true }); c.preventDefault(); c.initCustomEvent('a', true, true, 1); return [c.defaultPrevented, c.detail]");
expr("new CustomEvent('a').initCustomEvent()");
expr("new CustomEvent('a').initCustomEvent(Symbol())");
expr("new CustomEvent('a').initCustomEvent({ toString() { throw new RangeError('boom') } })");
body("var c = new CustomEvent('x'); c.initCustomEvent('p\\ud83d', true, false, 1); return [c.type.length, c.type.charCodeAt(1), c.type === 'p\\ud83d', c.detail]");
body("var c = new CustomEvent('k\\ud800'); return [c.type.length, c.type.charCodeAt(1)]");
// Despacho: o detail chega ao ouvinte; initCustomEvent durante o despacho não muda nada.
body("et.addEventListener('a', function (e) { log.push(e.detail, e instanceof CustomEvent, e.target === et, e.eventPhase) }); var r = et.dispatchEvent(new CustomEvent('a', { detail: 'dd' })); return [r, log]");
body("var c = new CustomEvent('a', { detail: 1 }); et.addEventListener('a', function (e) { e.initCustomEvent('zz', true, true, 9); log.push(e.type, e.detail, e.bubbles) }); et.dispatchEvent(c); return [log, c.type, c.detail]");
body("et.addEventListener('c', function (e) { e.preventDefault() }); var a = new CustomEvent('c', { cancelable: true, detail: 1 }), b = new CustomEvent('c', { detail: 1 }); return [et.dispatchEvent(a), a.defaultPrevented, et.dispatchEvent(b), b.defaultPrevented]");
body("var c = new CustomEvent('nest', { detail: 1 }); et.addEventListener('nest', function () { try { et.dispatchEvent(c) } catch (e) { log.push(e.constructor.name + '|' + e.name + '|' + e.message + '|' + e.code) } }); et.dispatchEvent(c); return log");
body("var k = 'z\\udc00'; var c = new CustomEvent(k); et.addEventListener(k, function () { try { et.dispatchEvent(c) } catch (e) { log.push(e.message.length, e.message.charCodeAt(12)) } }); et.dispatchEvent(c); return log");

// AbortController e AbortSignal, e a opção `signal` de addEventListener.
const AD = "var AP = AbortSignal.prototype, CP = AbortController.prototype, ac = new AbortController(), as = ac.signal;\n";
const aexpr = (code) => programs.push(HELPER + AD + `try { R = S(${code}) } catch (e) { R = E(e) }`);
const abody = (code) => aexpr(`(function(){ ${code} })()`);
for (const C of ["AbortController", "AbortSignal"]) {
  aexpr(`D(Object.getOwnPropertyDescriptor(globalThis, '${C}'))`);
  aexpr(`[${C}.length, ${C}.name, ${C}.toString(), typeof ${C}]`);
  aexpr(`Object.getPrototypeOf(${C}) === Function.prototype`);
  aexpr(`Object.getPrototypeOf(${C}) === EventTarget`);
  aexpr(`Object.getPrototypeOf(${C}.prototype) === Object.prototype`);
  aexpr(`Object.getPrototypeOf(${C}.prototype) === EventTarget.prototype`);
  aexpr(`Object.prototype.toString.call(${C}.prototype)`);
  aexpr(`Object.keys(${C}.prototype)`);
  aexpr(`Reflect.ownKeys(${C}.prototype).map(String)`);
}
aexpr("[X(AbortController), X(CP), X(AbortSignal), X(AP)]");
aexpr("[Reflect.ownKeys(ac), Reflect.ownKeys(as), String(ac), String(as), as === ac.signal, as instanceof AbortSignal, as instanceof EventTarget, ac instanceof EventTarget]");
aexpr("[as.aborted, as.reason, as.onabort]");
aexpr("AbortController()");
aexpr("AbortSignal()");
aexpr("new AbortSignal()");
aexpr("AbortSignal.call({})");
// Brand check.
for (const g of ["aborted", "reason", "onabort"]) aexpr(`Object.getOwnPropertyDescriptor(AP, '${g}').get.call({})`);
aexpr("Object.getOwnPropertyDescriptor(AP, 'onabort').set.call({}, null)");
aexpr("Object.getOwnPropertyDescriptor(CP, 'signal').get.call({})");
aexpr("AP.throwIfAborted.call({})");
aexpr("CP.abort.call({})");
aexpr("AbortController.prototype.abort.call(as)");
aexpr("(function () { var f = new AbortController().abort; try { f() } catch (e) { return E(e) } })()");
abody("class Z extends AbortController {}; var z = new Z(); return [z.signal instanceof AbortSignal, z instanceof Z, Object.getPrototypeOf(z) === Z.prototype]");
// abort() e reason.
abody("ac.abort(); var r = as.reason; return [as.aborted, r.name, r.message, r.code, r instanceof DOMException, r.constructor.name]");
abody("var o = {}; ac.abort(o); return [as.aborted, as.reason === o]");
abody("ac.abort(5); return as.reason");
abody("ac.abort(null); return [as.aborted, as.reason]");
abody("ac.abort(undefined); return as.reason.name");
abody("ac.abort('a'); ac.abort('b'); return as.reason");
abody("ac.abort('a'); var r = ac.abort('b'); return r");
// Evento abort.
abody("as.onabort = function (e) { log.push(['on', e.type, e.target === as, e.currentTarget === as, this === as, e.isTrusted, e.bubbles, e.cancelable, e.composed, e.constructor.name, e.eventPhase, as.aborted, as.reason]) }; as.addEventListener('abort', function () { log.push('add') }); ac.abort('r'); ac.abort('s'); return log");
abody("as.addEventListener('abort', function () { as.removeEventListener; ac.abort('again'); log.push(as.reason) }); ac.abort('first'); return log");
abody("as.addEventListener('abort', function (e) { log.push(e.defaultPrevented, e.isTrusted) }); ac.abort(); return [log, as.dispatchEvent(new Event('abort')), new Event('abort').isTrusted]");
// onabort.
abody("var a = as.onabort; as.onabort = 5; var b = as.onabort; var f = function () {}; as.onabort = f; var c = as.onabort === f; as.onabort = null; return [a, b, c, as.onabort]");
abody("as.addEventListener('abort', function () { log.push('a') }); as.onabort = function () { log.push('on1') }; as.addEventListener('abort', function () { log.push('b') }); as.onabort = function () { log.push('on2') }; ac.abort(); return log");
abody("as.onabort = function () { log.push('on') }; as.onabort = null; ac.abort(); return [log, as.onabort]");
abody("var o = { handleEvent() { log.push(1) } }; as.onabort = o; ac.abort(); return [as.onabort === o, log]");
abody("as.onabort = function () { log.push('on') }; as.onabort = 'str'; ac.abort(); return [log, as.onabort]");
abody("var f = function () { log.push('x') }; as.onabort = f; as.addEventListener('abort', f); ac.abort(); return log");
abody("function f() { log.push('x') } as.addEventListener('abort', f); as.addEventListener('abort', f); as.onabort = f; as.onabort = f; ac.abort(); return log");
// throwIfAborted.
abody("return [as.throwIfAborted()]");
abody("ac.abort(); try { as.throwIfAborted() } catch (e) { return [e.name, e.message, e.code, e === as.reason] }");
abody("ac.abort(7); try { as.throwIfAborted() } catch (e) { return e }");
// AbortSignal.abort.
aexpr("(function () { var s = AbortSignal.abort(); return [s.aborted, s.reason.name, s.reason.message, s instanceof AbortSignal, s.throwIfAborted === AP.throwIfAborted] })()");
aexpr("AbortSignal.abort('x').reason");
aexpr("AbortSignal.abort(null).reason");
aexpr("AbortSignal.abort.call(1).aborted");
aexpr("[AbortSignal.abort.length, AbortSignal.any.length, AP.throwIfAborted.length, CP.abort.length]");
abody("var s = AbortSignal.abort(); s.addEventListener('abort', function () { log.push(1) }); return [log, s.aborted]");
// AbortSignal.any.
aexpr("AbortSignal.any()");
aexpr("AbortSignal.any(5)");
aexpr("AbortSignal.any({})");
aexpr("AbortSignal.any(null)");
aexpr("AbortSignal.any(undefined)");
aexpr("AbortSignal.any(true)");
aexpr("AbortSignal.any('ab')");
aexpr("AbortSignal.any({ [Symbol.iterator]: 1 })");
abody("ac.abort(); ac.abort('x'); return [as.reason.name, as.reason.message, as.reason.code, as.reason instanceof DOMException, Object.prototype.toString.call(as.reason)]");
abody("as.onabort = function () { log.push('on') }; as.addEventListener('abort', function () { log.push('ev') }); ac.abort(); return log");
abody("ac.abort(undefined); return [as.reason.name]");
abody("ac.abort(null); return [as.reason, as.aborted]");
abody("var c5 = new AbortController(), d = AbortSignal.any([c5.signal]); d.onabort = function () { log.push('any') }; c5.signal.addEventListener('abort', function () { log.push('src') }); c5.abort('R'); return [log, d.aborted, d.reason]");
aexpr("Object.keys(AbortSignal)");
aexpr("(function () { try { AbortSignal.timeout(-1) } catch (e) { return [e.name, e.message] } })()");
aexpr("(function () { try { AbortSignal.timeout() } catch (e) { return [e.name, e.message, e.code] } })()");
aexpr("AbortSignal.any([1])");
aexpr("AbortSignal.any([as, {}])");
aexpr("AbortSignal.any([as, 'x'])");
aexpr("AbortSignal.any([as, null])");
abody("var s = AbortSignal.any([]); return [s.aborted, s.reason, s instanceof AbortSignal]");
abody("var a2 = new AbortController(); var s = AbortSignal.any([as, a2.signal]); var before = s.aborted; s.onabort = function (e) { log.push(['on', e.isTrusted, e.target === s]) }; a2.abort('why'); return [before, s.aborted, s.reason, as.aborted, log]");
abody("var s = AbortSignal.any([new AbortController().signal, AbortSignal.abort('pre')]); return [s.aborted, s.reason]");
abody("var s = AbortSignal.any(new Set([as])); ac.abort(1); return [s.aborted, s.reason]");
abody("var a = AbortSignal.any([as]), b = AbortSignal.any([a]); ac.abort(3); return [a.reason, b.reason, b.aborted]");
abody("as.addEventListener('abort', function () { log.push('src') }); var d = AbortSignal.any([as]); d.addEventListener('abort', function () { log.push('dep') }); ac.abort(); return log");
abody("var d = AbortSignal.any([as]); ac.abort(); d.onabort = function () { log.push('late') }; return [log, d.aborted]");
// Ordem de despacho entre dependentes de vários níveis (signalAbort: só as raízes guardam dependentes, na ordem de criação).
abody("var s = as, d1 = AbortSignal.any([s]), d2 = AbortSignal.any([s]), dd1 = AbortSignal.any([d1]), dd2 = AbortSignal.any([d2]), d3 = AbortSignal.any([s]); [['s', s], ['d1', d1], ['d2', d2], ['dd1', dd1], ['dd2', dd2], ['d3', d3]].forEach(function (p) { p[1].addEventListener('abort', function () { log.push(p[0] + ':' + String(p[1].reason)) }) }); ac.abort('R'); return log");
abody("var e1 = AbortSignal.any([as]), ee = AbortSignal.any([e1]), e2 = AbortSignal.any([as]); [['t', as], ['e1', e1], ['ee', ee], ['e2', e2]].forEach(function (p) { p[1].addEventListener('abort', function () { log.push(p[0] + ':' + [as.aborted, e1.aborted, ee.aborted, e2.aborted].join()) }) }); ac.abort(); return log");
abody("var c2 = new AbortController(), m = AbortSignal.any([as, c2.signal]), n1 = AbortSignal.any([m, as]); m.addEventListener('abort', function () { log.push('m') }); n1.addEventListener('abort', function () { log.push('n1') }); as.addEventListener('abort', function () { log.push('c1') }); ac.abort('x'); c2.abort('y'); return [log, m.reason, n1.reason]");
abody("var g = AbortSignal.any([as]), h = AbortSignal.any([g]); as.addEventListener('abort', function () { log.push('f') }); g.addEventListener('abort', function () { log.push('g'); throw new Error('boom') }); h.addEventListener('abort', function () { log.push('h') }); ac.abort(); return [log, h.aborted]");
abody("var late; as.addEventListener('abort', function () { late = AbortSignal.any([as]); log.push('k:' + late.aborted) }); var k2 = AbortSignal.any([as]); k2.addEventListener('abort', function () { log.push('k2') }); ac.abort(); return [log, late.aborted, late.reason === as.reason]");
abody("var p1 = AbortSignal.any([as]), p2 = AbortSignal.any([p1]); as.addEventListener('abort', function () { log.push('p:' + [p1.aborted, p2.aborted]) }); p1.addEventListener('abort', function () { log.push('p1:' + [p2.aborted]) }); ac.abort(); return log");
abody("var o = { handleEvent() { log.push('obj') } }; var d = AbortSignal.any([as]); d.onabort = function () { log.push('on') }; d.addEventListener('abort', o); as.onabort = function () { log.push('son') }; ac.abort('z'); return [log, d.reason]");
// Opção signal de addEventListener.
abody("et.addEventListener('a', function () { log.push(1) }, { signal: as }); et.dispatchEvent(new Event('a')); ac.abort(); et.dispatchEvent(new Event('a')); return log");
abody("et.addEventListener('a', function () { log.push(1) }, { signal: AbortSignal.abort() }); et.dispatchEvent(new Event('a')); return log");
abody("var f = function () { log.push(1) }; et.addEventListener('a', f, { signal: as }); et.addEventListener('a', f, { signal: as }); et.dispatchEvent(new Event('a')); return log");
abody("et.addEventListener('a', function () { log.push(1) }, { signal: as, once: true }); et.dispatchEvent(new Event('a')); et.dispatchEvent(new Event('a')); return log");
abody("var f = function () { log.push(1) }; et.addEventListener('a', f, { signal: as }); et.removeEventListener('a', f); ac.abort(); et.addEventListener('a', f); et.dispatchEvent(new Event('a')); return log");
abody("var e2 = new EventTarget(); et.addEventListener('a', function () { log.push('et') }, { signal: as }); e2.addEventListener('a', function () { log.push('e2') }, { signal: as }); ac.abort(); et.dispatchEvent(new Event('a')); e2.dispatchEvent(new Event('a')); return log");
abody("et.addEventListener('a', function () { log.push('x') }, { signal: as }); as.addEventListener('abort', function () { log.push('abort-ev') }); ac.abort(); return log");
for (const v of ["5", "{}", "null", "'x'", "undefined", "new Event('x')", "new EventTarget()"]) {
  aexpr(`(function () { et.addEventListener('a', function () {}, { signal: ${v} }); return 'ok' })()`);
}
abody("var l = []; et.addEventListener('a', function () {}, { get capture() { l.push('c'); return 0 }, get once() { l.push('o'); return 0 }, get passive() { l.push('p'); return 0 }, get signal() { l.push('s'); return undefined } }); return l");

// AbortSignal.timeout: forma, validação de ms, e os casos que dependem do laço de eventos (`loopCase`: R é gravado num
// timer de 60 ms, ou por `tail`, e o gerador espera o laço esvaziar antes de ler).
aexpr("(function () { var d = Object.getOwnPropertyDescriptor(AbortSignal, 'timeout'); return [D(d), F(d.value), 'prototype' in d.value, Object.getPrototypeOf(d.value) === Function.prototype, Object.getOwnPropertyNames(d.value)] })()");
aexpr("AbortSignal.timeout()");
aexpr("(function () { var f = AbortSignal.timeout; try { f() } catch (e) { return E(e) } })()");
aexpr("AbortSignal.timeout.call(null, 1) instanceof AbortSignal");
for (const v of [
  "-1", "NaN", "'x'", "'5'", "2**53", "2**53+2", "Infinity", "-Infinity", "undefined", "null", "true", "false", "{}", "[]", "[3]", "1.5", "0", "-0", "-0.5",
  "2**31", "2**32", "Number.MAX_SAFE_INTEGER", "1e300", "BigInt(1)", "Symbol()", "{ valueOf() { throw new RangeError('v') } }", "{ valueOf() { return 7 } }",
]) {
  aexpr(`(function () { try { var s = AbortSignal.timeout(${v}); return ['ok', s instanceof AbortSignal, s.aborted, s.reason, Reflect.ownKeys(s), Object.getPrototypeOf(s) === AP] } catch (e) { return E(e) } })()`);
}
// ErrorEvent, MessageEvent, CloseEvent: forma, init dict (ordem de leitura e conversões) e brand check.
for (const C of ["ErrorEvent", "MessageEvent", "CloseEvent"]) {
  expr(`D(Object.getOwnPropertyDescriptor(globalThis, '${C}'))`);
  expr(`[${C}.length, ${C}.name, typeof ${C}, ${C}.toString()]`);
  expr(`Object.getPrototypeOf(${C}) === Event`);
  expr(`Object.getPrototypeOf(${C}.prototype) === Event.prototype`);
  expr(`X(${C})`);
  expr(`X(${C}.prototype)`);
  expr(`X(new ${C}('x'))`);
  expr(`Object.keys(new ${C}('x'))`);
  expr(`Object.prototype.toString.call(new ${C}('x'))`);
  expr(`${C}()`);
  expr(`new ${C}()`);
  expr(`new ${C}('a', 5)`);
  expr(`new ${C}('a', null).type`);
  expr(`(function(){ var e = new ${C}('t', { bubbles: 1, cancelable: 1, composed: 1 }); return [e.type, e.bubbles, e.cancelable, e.composed, e.isTrusted, e instanceof Event, e instanceof ${C}, e.eventPhase, e.target] })()`);
  expr(`(function(){ class X extends ${C} {} var x = new X('a'); return [x.type, Object.getPrototypeOf(x) === X.prototype, x instanceof ${C}, x instanceof Event] })()`);
  expr(`(function(){ var e = new ${C}('x'), t = new EventTarget(), r = []; t.addEventListener('x', function (ev) { r.push(ev === e, ev.target === t, ev.eventPhase) }); return [t.dispatchEvent(e), r] })()`);
  expr(`Object.getOwnPropertyDescriptor(${C}.prototype, Reflect.ownKeys(${C}.prototype)[1]).get.call({})`);
  expr(`Object.getOwnPropertyDescriptor(${C}.prototype, Reflect.ownKeys(${C}.prototype)[1]).get.call(new Event('a'))`);
  expr(`Object.getOwnPropertyDescriptor(${C}.prototype, Reflect.ownKeys(${C}.prototype)[1]).get.call(new CustomEvent('a'))`);
  expr(`Object.getOwnPropertyDescriptor(${C}.prototype, Reflect.ownKeys(${C}.prototype)[1]).get.call(${C}.prototype)`);
  expr(`(function(){ var r = []; new ${C}('x', { get bubbles() { r.push('b') }, get cancelable() { r.push('c') }, get composed() { r.push('d') }, get message() { r.push('message') }, get filename() { r.push('filename') }, get lineno() { r.push('lineno') }, get colno() { r.push('colno') }, get error() { r.push('error') }, get data() { r.push('data') }, get origin() { r.push('origin') }, get lastEventId() { r.push('lastEventId') }, get source() { r.push('source') }, get ports() { r.push('ports') }, get wasClean() { r.push('wasClean') }, get code() { r.push('code') }, get reason() { r.push('reason') } }); return r })()`);
}
expr("(function(){ var e = new ErrorEvent('x'); return [e.message, e.filename, e.lineno, e.colno, e.error, e.bubbles] })()");
expr("(function(){ var err = new Error('q'); var e = new ErrorEvent('x', { message: 5, filename: { toString() { return 'f' } }, lineno: '7', colno: -1.5, error: err }); return [e.message, e.filename, e.lineno, e.colno, e.error === err] })()");
expr("new ErrorEvent('x', { lineno: Symbol() })");
expr("new ErrorEvent('x', { message: Symbol() })");
expr("new ErrorEvent('x', { filename: { toString() { throw new RangeError('fn') } } })");
expr("(function(){ var e = new ErrorEvent('x', { lineno: 4294967297, colno: -1 }); return [e.lineno, e.colno] })()");
expr("[new ErrorEvent('x', { lineno: NaN, colno: Infinity }).lineno, new ErrorEvent('x', { lineno: 1e20 }).lineno, new ErrorEvent('x', { lineno: null }).lineno]");
expr("[new ErrorEvent('x', { error: undefined }).error, new ErrorEvent('x', { error: null }).error, new ErrorEvent('x', { error: 0 }).error]");
expr("(function(){ var o = {}; return new ErrorEvent('x', { error: o }).error === o })()");
expr("(function(){ class X extends ErrorEvent { constructor() { super('a', { message: 'm' }); this.v = 1 } } var x = new X(); return [x.message, x.v, Object.keys(x)] })()");
expr("(function(){ var e = new MessageEvent('x'); return [e.origin, e.lastEventId, e.source, e.data, e.ports, Object.isFrozen(e.ports), Array.isArray(e.ports), e.ports === e.ports, e.ports.length] })()");
expr("(function(){ var d = { a: 1 }; var e = new MessageEvent('x', { data: d, origin: 5, lastEventId: null, source: null, ports: [] }); return [e.data === d, e.origin, e.lastEventId, e.source, e.ports] })()");
expr("new MessageEvent('x', { source: {} })");
expr("new MessageEvent('x', { source: 5 })");
expr("new MessageEvent('x', { ports: 5 })");
expr("new MessageEvent('x', { ports: [1] })");
expr("new MessageEvent('x', { ports: { length: 0 } })");
expr("new MessageEvent('x', { ports: { a: 1, b: { c: [1, { d: 2 }] }, e: 's' } })");
expr("new MessageEvent('x', { ports: { a: [1,2,3,4,5,6,7,8,9,10,11,12] } })");
expr("new MessageEvent('x', { ports: function f() {} })");
expr("new MessageEvent('x', { ports: 'abc' })");
expr("new MessageEvent('x', { ports: null })");
expr("new MessageEvent('x', { ports: true })");
expr("new MessageEvent('x', { ports: {} })");
expr("new MessageEvent('x', { ports: Object.create(null) })");
expr("new MessageEvent('x', { ports: ['a'] })");
expr("new MessageEvent('x', { ports: [{ a: 1 }] })");
expr("new MessageEvent('x', { source: { a: 1 } })");
expr("new MessageEvent('x', { source: 'abc' })");
expr("new MessageEvent('x', { ports: new Map([[1, 2]]) })");
expr("new MessageEvent('x', { ports: new Map })");
expr("new MessageEvent('x', { ports: new Set([5]) })");
expr("new MessageEvent('x', { ports: new Set })");
expr("new MessageEvent('x', { ports: (function*(){})() })");
expr("new MessageEvent('x', { ports: (function*(){ yield 7 })() })");
expr("new MessageEvent('x', { ports: { [Symbol.iterator]() { throw new RangeError('boom') } } })");
expr("new MessageEvent('x', { ports: { [Symbol.iterator]() { return { next() { throw new RangeError('next') } } } } })");
expr("new MessageEvent('x', { ports: { [Symbol.iterator]: 5 } })");
expr("(function(){ var l = Error.stackTraceLimit; Error.stackTraceLimit = 0; var x = new Error('x'); Error.stackTraceLimit = l; return new MessageEvent('x', { ports: [x] }) })()");
expr("(function(){ var l = Error.stackTraceLimit; Error.stackTraceLimit = 0; var x = new Error('x'); Error.stackTraceLimit = l; return new MessageEvent('x', { source: x }) })()");
// Os mesmos casos com a pilha natural (sem `Error.stackTraceLimit = 0`): o fonte roda como `/app/main.js` (como
// `gen-uncaught-golden.js`), o resultado vai para o stderr e o golden próprio `event_target_main_bun.tsv` guarda o stderr
// inteiro e o código de saída. Os dois casos de `stackTraceLimit = 0` acima ficam no golden principal.
const mainSources = [];
const mainCase = (code) => mainSources.push(HELPER + `var R;\ntry { R = S(${code}) } catch (e) { R = E(e) }\nconsole.error(R);\n`);
mainCase("new MessageEvent('x', { ports: [new Error('x')] })");
mainCase("new MessageEvent('x', { source: new Error('x') })");
mainCase("(function(){ var l = Error.stackTraceLimit; Error.stackTraceLimit = 0; var x = new Error('x'); Error.stackTraceLimit = l; return new MessageEvent('x', { ports: [x] }) })()");
mainCase("(function(){ var l = Error.stackTraceLimit; Error.stackTraceLimit = 0; var x = new Error('x'); Error.stackTraceLimit = l; return new MessageEvent('x', { source: x }) })()");
expr("[new MessageEvent('x', { data: undefined }).data, new MessageEvent('x', { data: 0 }).data, new MessageEvent('x', { data: null }).data]");
expr("(function(){ var e = new MessageEvent('x'); return [e.initMessageEvent('y', true, true, 5, 'o', 'l', null, []), e.type, e.bubbles, e.cancelable, e.data, e.origin, e.lastEventId, e.source] })()");
expr("(function(){ var e = new MessageEvent('x', { data: 1 }); e.initMessageEvent('y'); return [e.type, e.data, e.origin, e.lastEventId, e.bubbles] })()");
expr("new MessageEvent('x').initMessageEvent()");
expr("MessageEvent.prototype.initMessageEvent.call({}, 'a')");
expr("MessageEvent.prototype.initMessageEvent.call(new Event('a'), 'a')");
expr("(function(){ var e = new MessageEvent('x', { data: 1 }), t = new EventTarget(); t.addEventListener('x', function (ev) { ev.initMessageEvent('y', true, true, 2, 'o') }); t.dispatchEvent(e); return [e.type, e.data, e.origin, e.bubbles] })()");
expr("[new CloseEvent('x').wasClean, new CloseEvent('x').code, new CloseEvent('x').reason]");
expr("(function(){ var e = new CloseEvent('x', { wasClean: 1, code: 70000, reason: 5 }); return [e.wasClean, e.code, e.reason] })()");
expr("[new CloseEvent('x', { code: -1 }).code, new CloseEvent('x', { code: 1.9 }).code, new CloseEvent('x', { code: '12' }).code, new CloseEvent('x', { code: NaN }).code, new CloseEvent('x', { code: 65536 }).code]");
expr("new CloseEvent('x', { reason: Symbol() })");
expr("new CloseEvent('x', { code: Symbol() })");
expr("new CloseEvent('x', { code: { valueOf() { throw new RangeError('c') } } })");
expr("[new CloseEvent('x', { wasClean: 'false' }).wasClean, new CloseEvent('x', { wasClean: 0 }).wasClean]");
expr("(function(){ var e = new CloseEvent('x', { reason: 'r' }); e.code = 5; e.reason = 'z'; return [e.code, e.reason] })()");
// Subclasses em JS de Event e EventTarget.
expr("(function(){ class X extends EventTarget { constructor() { super(); this.v = 1 } } var x = new X(), r = []; x.addEventListener('a', function (e) { r.push(this === x, e.target === x, this.v) }); return [x.dispatchEvent(new Event('a')), r, Object.keys(x), Reflect.ownKeys(x)] })()");
expr("(function(){ class X extends EventTarget { dispatchEvent(e) { return 'own:' + super.dispatchEvent(e) } } var x = new X(); return [x.dispatchEvent(new Event('a')), x instanceof EventTarget, Object.getPrototypeOf(X) === EventTarget] })()");
expr("(function(){ class X extends EventTarget { addEventListener(t, f) { log.push(t); return super.addEventListener(t, f) } } var x = new X(); x.addEventListener('a', function () { log.push('called') }); x.dispatchEvent(new Event('a')); return log })()");
expr("(function(){ class X extends EventTarget {} var a = new X(), b = new X(); var r = []; a.addEventListener('t', function () { r.push('a') }); b.addEventListener('t', function () { r.push('b') }); b.dispatchEvent(new Event('t')); return r })()");
expr("(function(){ class X extends EventTarget { constructor() { this.x = 1; super() } } new X() })()");
expr("(function(){ class X extends EventTarget { constructor() { } } new X() })()");
expr("(function(){ class X extends EventTarget { constructor() { super(); super() } } new X() })()");
expr("(function(){ class X extends EventTarget { constructor() { return {} } } var x = new X(); return [x instanceof EventTarget, x instanceof X] })()");
expr("(function(){ class X extends EventTarget { constructor() { return {} } } var x = new X(); x.addEventListener('a', function () {}) })()");
expr("(function(){ class E2 extends Event { constructor(t) { super(t, { cancelable: true }); this.extra = 5 } } var e = new E2('z'); return [Object.keys(e), Reflect.ownKeys(e), e.cancelable, e.extra, e.type] })()");
expr("(function(){ class E2 extends Event { constructor(t) { super(t, { cancelable: true }); this.extra = 5 } } var e = new E2('z'), t = new EventTarget(), r = []; t.addEventListener('z', function (ev) { ev.preventDefault(); r.push(ev === e, ev.extra) }); return [t.dispatchEvent(e), r, e.defaultPrevented] })()");
expr("(function(){ class E2 extends Event { constructor(t) { this.x = 1; super(t) } } new E2('z') })()");
expr("(function(){ class E2 extends Event { constructor() { super() } } new E2() })()");
expr("(function(){ class E2 extends Event { get type() { return 'over' } } return [new E2('real').type, Object.getOwnPropertyDescriptor(Event.prototype, 'type').get.call(new E2('real'))] })()");
expr("(function(){ class E2 extends CustomEvent {} var e = new E2('z', { detail: 3 }); return [e.detail, e instanceof CustomEvent, e instanceof Event] })()");
expr("(function(){ class T extends EventTarget { constructor() { super(); this.on = null } } class E2 extends Event {} var t = new T(), r = []; t.addEventListener('q', { handleEvent(e) { r.push(this === undefined, e instanceof E2) } }); t.dispatchEvent(new E2('q')); return r })()");
expr("(function(){ function F() { return Reflect.construct(EventTarget, [], F) } F.prototype = Object.create(EventTarget.prototype); var f = new F(); return [f instanceof F, f instanceof EventTarget, typeof f.dispatchEvent] })()");
expr("(function(){ function F() { return Reflect.construct(Event, ['a'], F) } F.prototype = Object.create(Event.prototype); var f = new F(); return [f instanceof F, f.type] })()");
expr("(function(){ var o = Object.create(EventTarget.prototype); return o.dispatchEvent(new Event('a')) })()");
const loopCases = new Set();
const loopCase =(code, tail) => {
  const source = HELPER + AD + `try { (function(){ ${code} })(); ${tail === undefined ? "setTimeout(function () { R = S(log) }, 60);" : tail} } catch (e) { R = E(e) }`;
  loopCases.add(source);
  programs.push(source);
};
const TM = "var mk = function (n, ms) { var s = AbortSignal.timeout(ms); s.addEventListener('abort', function () { log.push(n) }); return s };";
// Reason, evento e estado.
loopCase("var s = AbortSignal.timeout(5); var pre = [s.aborted, s.reason, Reflect.ownKeys(s)]; s.addEventListener('abort', function (e) { log.push(['ev', e.type, e.isTrusted, e.target === s, e.currentTarget === s, this === s, s.aborted, e.bubbles, e.cancelable, e.constructor.name]) }); s.onabort = function () { log.push('onabort') }; setTimeout(function () { var r = s.reason; log.push(pre, s.aborted, r.constructor.name, r.name, r.message, r.code, r instanceof DOMException, r instanceof Error, Reflect.ownKeys(r).map(String), Object.keys(r), typeof r.stack, r.stack, s.reason === r, String(r)); try { s.throwIfAborted() } catch (e) { log.push(e === r) } }, 30);");
loopCase("var s = AbortSignal.timeout(2), n = 0; s.addEventListener('abort', function () { n++; log.push(n) }); setTimeout(function () { log.push(s.aborted) }, 20);");
loopCase("var s = AbortSignal.timeout(1); s.onabort = function () { log.push('on') }; s.addEventListener('abort', function () { log.push('add') }); s.addEventListener('abort', function () { log.push('add2') }, { once: true });");
loopCase("var s = AbortSignal.timeout(1), n = 0; s.addEventListener('abort', function () { log.push(s.reason.name, ++n); if (n < 2) s.dispatchEvent(new Event('abort')) });");
loopCase("var et2 = new EventTarget(), s = AbortSignal.timeout(2); et2.addEventListener('a', function () { log.push(1) }, { signal: s }); et2.dispatchEvent(new Event('a')); setTimeout(function () { et2.dispatchEvent(new Event('a')) }, 15);");
// Dependentes (`any`) e mistura com abort manual.
loopCase("var sb = AbortSignal.timeout(2), dep = AbortSignal.any([sb]), dd = AbortSignal.any([dep]); dep.addEventListener('abort', function () { log.push('dep') }); dd.addEventListener('abort', function () { log.push('dd') }); sb.addEventListener('abort', function () { log.push('sb') }); setTimeout(function () { log.push(dep.aborted, dd.aborted, dep.reason === sb.reason, dd.reason === sb.reason, sb.reason.name) }, 20);");
loopCase("var c = new AbortController(), t = AbortSignal.timeout(3), dep = AbortSignal.any([c.signal, t]); dep.addEventListener('abort', function () { log.push('dep') }); t.addEventListener('abort', function () { log.push('t') }); c.abort('manual'); setTimeout(function () { log.push(dep.reason, t.aborted, t.reason.name) }, 20);");
loopCase("var t = AbortSignal.timeout(3), dep = AbortSignal.any([t]); t.addEventListener('abort', function () { log.push('t') }); AbortSignal.any([AbortSignal.abort('pre'), t]).addEventListener('abort', function () { log.push('never') }); setTimeout(function () { log.push(dep.aborted, dep.reason.name) }, 20);");
// Timeout como origem de `any`: ordem de despacho (origem, dependente, dependente do dependente), reason e mistura com controller.
loopCase("var sb = AbortSignal.timeout(2), dep = AbortSignal.any([sb]), dd = AbortSignal.any([dep]); dd.addEventListener('abort', function () { log.push('dd') }); dep.addEventListener('abort', function () { log.push('dep') }); sb.addEventListener('abort', function () { log.push('sb') }); dd.addEventListener('abort', function () { log.push('dd2') }); dep.onabort = function () { log.push('dep.onabort') }; sb.onabort = function () { log.push('sb.onabort') };");
loopCase("var sb = AbortSignal.timeout(2), dep = AbortSignal.any([sb]), dd = AbortSignal.any([dep, sb]); sb.addEventListener('abort', function () { log.push(['sb', dep.aborted, dd.aborted]) }); dep.addEventListener('abort', function () { log.push(['dep', sb.aborted, dd.aborted]) }); dd.addEventListener('abort', function () { log.push(['dd', sb.aborted, dep.aborted, dd.reason === sb.reason]) });");
loopCase("var sb = AbortSignal.timeout(2), dep = AbortSignal.any([sb]), dd = AbortSignal.any([dep]); setTimeout(function () { var r = sb.reason; log.push(dep.reason === r, dd.reason === r, r.constructor.name, r.name, r.message, r.code, r instanceof DOMException, String(r), dep.reason.name, dd.reason.code, dd.aborted); try { dd.throwIfAborted() } catch (e) { log.push(e === r, e.name) } }, 30);");
loopCase("var sb = AbortSignal.timeout(2), dep = AbortSignal.any([sb]); dep.addEventListener('abort', function (e) { log.push([e.type, e.isTrusted, e.target === dep, e.currentTarget === dep, dep.reason.name, dep.reason.message, dep.reason.code]) });");
// Any criado depois do prazo, e any com origem já abortada por timeout.
loopCase("var sb = AbortSignal.timeout(1); setTimeout(function () { var dep = AbortSignal.any([sb]); log.push(dep.aborted, dep.reason === sb.reason, dep.reason.name); dep.addEventListener('abort', function () { log.push('never') }) }, 15);");
// Timeout e controller: o controller aborta antes do prazo.
loopCase("var c = new AbortController(), t = AbortSignal.timeout(3), dep = AbortSignal.any([c.signal, t]), dd = AbortSignal.any([dep]); dep.addEventListener('abort', function () { log.push('dep') }); dd.addEventListener('abort', function () { log.push('dd') }); t.addEventListener('abort', function () { log.push('t') }); c.signal.addEventListener('abort', function () { log.push('c') }); c.abort(); setTimeout(function () { log.push(dep.reason.name, dd.reason.name, dep.reason === c.signal.reason, t.aborted, t.reason.name, dep.aborted) }, 20);");
loopCase("var c = new AbortController(), t = AbortSignal.timeout(3), dep = AbortSignal.any([t, c.signal]); dep.addEventListener('abort', function () { log.push('dep') }); c.abort('manual'); setTimeout(function () { log.push(dep.reason, t.reason.name) }, 20);");
// O controller aborta depois do prazo: o timeout vence e o abort tardio não muda nada.
loopCase("var c = new AbortController(), t = AbortSignal.timeout(2), dep = AbortSignal.any([c.signal, t]); dep.addEventListener('abort', function () { log.push('dep') }); setTimeout(function () { c.abort('late'); log.push(dep.reason.name, c.signal.aborted, c.signal.reason, t.reason.name) }, 20);");
// Controller abortado antes de qualquer prazo, no mesmo tick da criação.
loopCase("var c = new AbortController(); c.abort('pre'); var t = AbortSignal.timeout(2), dep = AbortSignal.any([t, c.signal]); dep.addEventListener('abort', function () { log.push('never') }); t.addEventListener('abort', function () { log.push('t') }); setTimeout(function () { log.push(dep.aborted, dep.reason, t.aborted, t.reason.name) }, 20);");
// Dois timeouts na mesma fonte e prazos diferentes.
loopCase("var a = AbortSignal.timeout(3), b = AbortSignal.timeout(1), dep = AbortSignal.any([a, b]), dd = AbortSignal.any([dep, a]); a.addEventListener('abort', function () { log.push('a') }); b.addEventListener('abort', function () { log.push('b') }); dep.addEventListener('abort', function () { log.push('dep') }); dd.addEventListener('abort', function () { log.push('dd') }); setTimeout(function () { log.push(dep.reason === b.reason, dd.reason === b.reason, a.aborted) }, 20);");
// Ordem em relação a setTimeout, setImmediate e microtasks.
loopCase(TM + "var s1 = mk('s5', 5); setTimeout(function () { log.push('t5-after') }, 5); mk('s5b', 5); setTimeout(function () { log.push('t5-after2') }, 5); setTimeout(function () { log.push('t4') }, 4); setTimeout(function () { log.push('t6') }, 6);");
loopCase(TM + "setTimeout(function () { log.push('t0') }, 0); mk('s0', 0); setTimeout(function () { log.push('t1') }, 1); mk('s1', 1); setImmediate(function () { log.push('imm') }); Promise.resolve().then(function () { log.push('micro') }); log.push('sync');");
loopCase(TM + "setTimeout(function () { log.push('t2') }, 2); mk('s1.5', 1.5); mk('s2.9', 2.9); setTimeout(function () { log.push('t1') }, 1); mk(\"s'3'\", '3'); mk('s-0.5', -0.5); mk('sTrue', true); setTimeout(function () { log.push('t3') }, 3);");
loopCase(TM + "var a = +setTimeout(function () {}, 1); AbortSignal.timeout(1); var b = +setTimeout(function () {}, 1); log.push(b - a);");
loopCase(TM + "setImmediate(function () { log.push('imm'); }); mk('s0', 0);");
loopCase(TM + "setTimeout(function () { mk('s0', 0); setImmediate(function () { log.push('imm') }); setTimeout(function () { log.push('t0') }, 0); mk('s1', 1); setTimeout(function () { log.push('t1') }, 1) }, 2);");
loopCase(TM + "setImmediate(function () { log.push('imm'); mk('s0', 0); setTimeout(function () { log.push('t0') }, 0); setImmediate(function () { log.push('imm2') }) });");
loopCase(TM + "var t = setTimeout(function () { log.push('t3') }, 3); mk('s3', 3); clearTimeout(t); t = setTimeout(function () { log.push('t3b') }, 3);");
// O timer nativo não consome id. (Que ele não mantém o laço vivo é medido pelos casos de processo mais abaixo.)
loopCase("var s = AbortSignal.timeout(10); s.addEventListener('abort', function () { log.push('s10') }); setTimeout(function () { log.push('t20') }, 20);", "setTimeout(function () { R = S(log) }, 20);");
loopCase("var s = AbortSignal.timeout(5); s.addEventListener('abort', function () { log.push('s5'); setTimeout(function () { log.push('inner') }, 1) });");

// Casos de processo: cada um roda num bun próprio (`spawnSync`) e registra o que aconteceu até o processo sair, o que
// mede o `ref` do timer nativo (o `AbortSignal.timeout` não mantém o processo vivo). `R` é um getter de `S(log)` lido
// na saída do processo; o porte roda o laço até esvaziar e lê o mesmo getter, então o texto é o mesmo programa.
const processCases = new Set();
const processCase = (code) => {
  const source =
    HELPER + AD + `Object.defineProperty(globalThis, 'R', { configurable: true, get: function () { return S(log) } });\n(function(){ ${code} })();`;
  processCases.add(source);
  programs.push(source);
};
const runInOwnProcess = (source) => {
  const child =
    "globalThis.U = []; process.on('uncaughtException', function (e) { globalThis.U.push(String(e)) });" +
    "process.on('exit', function () { require('fs').writeSync(1, String(globalThis.R === undefined ? '<undefined>' : globalThis.R)) });" +
    "(0, eval)(process.env.EVENT_TARGET_CASE);";
  const done = spawnSync(process.execPath, ["-e", child], { env: { ...process.env, EVENT_TARGET_CASE: source }, encoding: "utf8", timeout: 10000 });
  if (done.status !== 0) throw new Error("caso de processo falhou: " + done.status + " " + done.stderr);
  return done.stdout;
};
const LISTEN = "var s = AbortSignal.timeout(50); s.addEventListener('abort', function () { log.push('abort') });";
processCase(LISTEN);
processCase(LISTEN + "setTimeout(function () {}, 0);");
processCase(LISTEN + "setTimeout(function () { log.push('t10') }, 10);");
processCase(LISTEN + "setTimeout(function () {}, 100);");
processCase(LISTEN + "setTimeout(function () { log.push('t100') }, 100);");
processCase(LISTEN + "setInterval(function () {}, 20).unref(); setTimeout(function () {}, 100).unref();");
processCase(LISTEN + "var t = setTimeout(function () {}, 100); t.unref(); t.ref();");
processCase(LISTEN + "var t = setTimeout(function () {}, 100); clearTimeout(t);");
processCase(LISTEN + "setTimeout(function () {}, 100).unref(); setImmediate(function () { log.push('imm') });");
processCase("var s = AbortSignal.timeout(0); s.addEventListener('abort', function () { log.push('abort') });");
processCase("var s = AbortSignal.timeout(0); s.addEventListener('abort', function () { log.push('abort') }); setImmediate(function () { log.push('imm') });");
processCase("var s = AbortSignal.timeout(5); s.addEventListener('abort', function () { log.push('s5'); setTimeout(function () { log.push('inner') }, 1) });");
processCase("var s = AbortSignal.timeout(5); s.addEventListener('abort', function () { log.push('s5'); setTimeout(function () { log.push('inner') }, 1) }); setTimeout(function () {}, 20);");
processCase("var d = AbortSignal.any([AbortSignal.timeout(5)]); d.addEventListener('abort', function () { log.push('dep') });");
processCase("var d = AbortSignal.any([AbortSignal.timeout(5)]); d.addEventListener('abort', function () { log.push('dep') }); setTimeout(function () {}, 20);");
processCase("var a = AbortSignal.timeout(5), b = AbortSignal.timeout(10); a.addEventListener('abort', function () { log.push('a') }); b.addEventListener('abort', function () { log.push('b') }); setTimeout(function () { log.push('t7') }, 7);");

(async () => {
  for (const source of programs) {
    const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
    if (processCases.has(source)) {
      emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(runInOwnProcess(sourceAscii)));
      continue;
    }
    (0, eval)("var R, U = []");
    (0, eval)(sourceAscii);
    if (loopCases.has(source)) await new Promise((resolve) => setTimeout(resolve, 150));
    emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
  }
  fs.writeFileSync(path.join(__dirname, "..", "tests", "golden", "event_target_main_bun.tsv"), mainSources.map((c) => runMain(c)).join("\n") + "\n");
})();
