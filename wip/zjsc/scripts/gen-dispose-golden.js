// Gera tests/golden/dispose_bun.tsv: gerenciamento explícito de recursos (DisposableStack, AsyncDisposableStack,
// SuppressedError, Symbol.dispose e Symbol.asyncDispose, a sintaxe `using` e `await using`) e os descritores de
// Symbol.iterator em Array, Map, Set, String e arrays tipados, mais os protótipos de Iterator, medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// O programa roda por `vm.runInThisContext` (ProgramExecutable do JSC puro); as microtarefas são esvaziadas antes de
// ler `R`, então os programas assíncronos gravam `R` ao fim da cadeia. Sem API de host.
// Uso: bun scripts/gen-dispose-golden.js > tests/golden/dispose_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const programs = [];
const add = body => programs.push(body);
const CATCH = "catch (e) { R = e.name + ': ' + e.message }";
// Programa síncrono com captura de exceção; o corpo atribui R.
const T = body => add(`try { ${body} } ${CATCH}`);
// Programa assíncrono: o corpo roda numa async arrow, e R é gravado dentro dela.
const A = body => add(`(async () => { try { ${body} } ${CATCH} })()`);
// Fonte compilada por eval indireto (escopo global), para os SyntaxError.
const S = src => add(`try { (0, eval)(${JSON.stringify(src)}); R = 'ok' } ${CATCH}`);
const J = "JSON.stringify";
// Descritor resumido de uma propriedade: tipo e flags.
const DESC = "(o, k) => { var d = Object.getOwnPropertyDescriptor(o, k); return d ? ('value' in d ? 'data' : 'accessor') + ':' + (d.writable ? 'w' : '-') + (d.enumerable ? 'e' : '-') + (d.configurable ? 'c' : '-') + (d.get ? ':get' : '') + (d.set ? ':set' : '') : 'none' }";

// ---- Símbolos conhecidos.
T("R = [typeof Symbol.dispose, typeof Symbol.asyncDispose, Symbol.dispose.toString(), Symbol.asyncDispose.toString()].join('|')");
T("R = [Symbol.dispose.description, Symbol.asyncDispose.description].join('|')");
T(`R = [(${DESC})(Symbol, 'dispose'), (${DESC})(Symbol, 'asyncDispose')].join('|')`);
T("R = [Symbol.keyFor(Symbol.dispose), Symbol.keyFor(Symbol.asyncDispose)].join('|')");
T("R = [Symbol.for('Symbol.dispose') === Symbol.dispose, Symbol.dispose === Symbol.asyncDispose].join()");
T("R = Object.getOwnPropertyNames(Symbol).filter(k => /dispose/i.test(k)).join()");
T("try { Symbol.dispose = 1; R = 'sloppy ' + typeof Symbol.dispose } catch (e) { R = e.name }");
T("'use strict'; Symbol.dispose = 1");
T("R = String(Object(Symbol.dispose)) + typeof Object(Symbol.asyncDispose)");
T("R = Symbol.dispose in Object.prototype");
T("R = [Symbol.dispose, Symbol.asyncDispose].map(s => typeof Function.prototype[s]).join()");

// ---- Descritores das classes.
for (const C of ["DisposableStack", "AsyncDisposableStack", "SuppressedError"]) {
  T(`R = [typeof ${C}, ${C}.name, ${C}.length].join()`);
  T(`R = (${DESC})(globalThis, '${C}')`);
  T(`R = [(${DESC})(${C}, 'prototype'), (${DESC})(${C}, 'name'), (${DESC})(${C}, 'length')].join()`);
  T(`R = Object.getOwnPropertyNames(${C}).sort().join()`);
  T(`R = Object.getOwnPropertySymbols(${C}).length`);
  T(`R = Object.getPrototypeOf(${C}) === ${C === "SuppressedError" ? "Error" : "Function.prototype"}`);
  T(`R = Object.getOwnPropertyNames(${C}.prototype).sort().join()`);
  T(`R = Object.getOwnPropertySymbols(${C}.prototype).map(String).join()`);
  T(`R = (${DESC})(${C}.prototype, Symbol.toStringTag) + '|' + ${C}.prototype[Symbol.toStringTag]`);
  T(`R = Object.getPrototypeOf(${C}.prototype) === ${C === "SuppressedError" ? "Error.prototype" : "Object.prototype"}`);
  T(`R = ${C}.prototype.constructor === ${C}`);
  T(`R = Object.prototype.toString.call(${C}.prototype)`);
  T(`R = String(${C}).replace(/\\s+/g, ' ')`);
  T(`R = (${DESC})(${C}.prototype, 'constructor')`);
  T(`R = ${J}(Reflect.ownKeys(${C}.prototype).map(String))`);
  T(`class X extends ${C} {}; R = [Object.getPrototypeOf(X) === ${C}, X.name].join()`);
}
for (const C of ["DisposableStack", "AsyncDisposableStack"]) {
  T(`${C}()`);
  T(`${C}.call({})`);
  T(`Reflect.construct(${C}, [], Object)`);
  T(`R = Object.prototype.toString.call(new ${C}())`);
  T(`R = Object.getOwnPropertyNames(new ${C}()).length`);
  T(`R = ${J}(new ${C}())`);
  T(`R = new ${C}(1, 2, 3).disposed`);
  T(`class X extends ${C} { constructor() { super(); this.tag = 7 } }; var x = new X(); R = [x.tag, x.disposed, x instanceof ${C}].join()`);
  T(`var s = new ${C}(); R = [s instanceof ${C}, Object.getPrototypeOf(s) === ${C}.prototype].join()`);
  T(`R = new ${C}().constructor === ${C}`);
  T(`R = typeof ${C}.prototype.disposed`);
  T(`R = ${C}.prototype.disposed`);
  T(`var d = Object.getOwnPropertyDescriptor(${C}.prototype, 'disposed'); R = [typeof d.get, d.set, d.get.name, d.get.length, d.enumerable, d.configurable].join()`);
  T(`var d = Object.getOwnPropertyDescriptor(${C}.prototype, 'disposed'); d.get.call({})`);
  T(`var d = Object.getOwnPropertyDescriptor(${C}.prototype, 'disposed'); d.get.call(new ${C}()); R = 'ok'`);
  T(`var d = Object.getOwnPropertyDescriptor(${C}.prototype, 'disposed'); d.get.call(undefined)`);
  T(`var s = new ${C}(); s.disposed = true; R = s.disposed`);
  T(`'use strict'; var s = new ${C}(); s.disposed = true`);
  T(`var s = new ${C}(); R = Object.keys(s).length + ':' + ('disposed' in s) + ':' + s.hasOwnProperty('disposed')`);
  for (const m of ["use", "adopt", "defer", "move"]) {
    T(`var p = ${C}.prototype; R = [p.${m}.name, p.${m}.length, (${DESC})(p, '${m}')].join()`);
    T(`${C}.prototype.${m}.call({})`);
    T(`${C}.prototype.${m}.call(undefined)`);
    T(`${C}.prototype.${m}.call(${C}.prototype)`);
    T(`${C}.prototype.${m}.call(new ${C === "DisposableStack" ? "AsyncDisposableStack" : "DisposableStack"}())`);
    T(`R = typeof new ${C}().${m}.prototype`);
    T(`new new ${C}().${m}()`);
  }
  const dn = C === "DisposableStack" ? "dispose" : "disposeAsync";
  T(`var p = ${C}.prototype; R = [p.${dn}.name, p.${dn}.length, (${DESC})(p, '${dn}')].join()`);
  T(`var p = ${C}.prototype; R = (p[${C === "DisposableStack" ? "Symbol.dispose" : "Symbol.asyncDispose"}] === p.${dn}) + ':' + p[${C === "DisposableStack" ? "Symbol.dispose" : "Symbol.asyncDispose"}].name + ':' + (${DESC})(p, ${C === "DisposableStack" ? "Symbol.dispose" : "Symbol.asyncDispose"})`);
  T(`R = ${J}(Object.getOwnPropertyNames(${C}.prototype).sort())`);
}
T("R = typeof DisposableStack.prototype.disposeAsync + typeof AsyncDisposableStack.prototype.dispose");
T("R = typeof DisposableStack.prototype[Symbol.asyncDispose] + typeof AsyncDisposableStack.prototype[Symbol.dispose]");

// ---- DisposableStack.use: valores aceitos e rejeitados.
const useValues = [
  "null", "undefined", "1", "'str'", "true", "Symbol()", "10n", "{}", "[]", "function () {}", "() => {}",
  "{ [Symbol.dispose]: 1 }", "{ [Symbol.dispose]: null }", "{ [Symbol.dispose]: undefined }", "{ [Symbol.dispose]: {} }",
  "{ [Symbol.dispose]() {} }", "{ [Symbol.asyncDispose]() {} }", "{ dispose() {} }", "{ get [Symbol.dispose]() { return () => {} } }",
  "{ get [Symbol.dispose]() { throw new RangeError('getter') } }", "Object.create({ [Symbol.dispose]() {} })",
  "new Proxy({}, { get(t, k) { return k === Symbol.dispose ? () => {} : undefined } })", "Object('s')", "new String('x')",
  "Object.assign(function () {}, { [Symbol.dispose]() {} })", "Object.create(null)", "{ [Symbol.dispose]: class {} }",
  "{ [Symbol.dispose]: async function () {} }", "{ [Symbol.dispose]: function* () {} }", "{ [Symbol.dispose]: 'dispose' }",
];
for (const v of useValues) {
  T(`var s = new DisposableStack(); var r = s.use(${v}); R = [r === undefined ? 'u' : r === null ? 'n' : typeof r, s.disposed].join()`);
  T(`var s = new DisposableStack(); var o = ${v}; var r = s.use(o); R = String(r === o)`);
}
for (const v of ["null", "undefined", "{}", "1", "{ [Symbol.dispose]: 1 }", "{ [Symbol.asyncDispose]() {} }"]) {
  A(`var s = new AsyncDisposableStack(); var r = s.use(${v}); await s.disposeAsync(); R = [typeof r, s.disposed].join()`);
}

// ---- adopt, defer, move.
const adoptCallbacks = ["undefined", "null", "1", "{}", "'f'", "function () {}", "() => {}", "class {}", "async () => {}", "function* () {}", "Symbol()"];
for (const v of adoptCallbacks) {
  T(`var s = new DisposableStack(); R = String(s.adopt(1, ${v}))`);
  T(`var s = new DisposableStack(); s.defer(${v}); R = 'deferred'`);
}
T("var s = new DisposableStack(); s.adopt(1)");
T("var s = new DisposableStack(); s.defer()");
T("var s = new DisposableStack(); R = [s.adopt(5, () => {}), s.adopt(undefined, () => {}), s.adopt(null, () => {})].join()");
T("var s = new DisposableStack(); var o = {}; R = String(s.adopt(o, () => {}) === o)");
T("var s = new DisposableStack(); R = String(s.defer(() => {}))");
T("var log = []; var s = new DisposableStack(); s.adopt('v', function (x) { log.push([x, arguments.length, this === undefined ? 'u' : typeof this].join(':')) }); s.dispose(); R = log.join()");
T("var log = []; var s = new DisposableStack(); s.defer(function () { log.push([arguments.length, this === undefined ? 'u' : typeof this].join(':')) }); s.dispose(); R = log.join()");
T("var log = []; var s = new DisposableStack(); s.defer(() => log.push(1)); var m = s.move(); R = [s.disposed, m.disposed, log.length, m instanceof DisposableStack, m === s].join(); m.dispose(); R += '|' + log.length");
T("var s = new DisposableStack(); s.dispose(); s.move()");
T("var log = []; var s = new DisposableStack(); s.defer(() => log.push('a')); var m = s.move(); s.dispose(); R = log.length + ':' + s.disposed + ':' + m.disposed");
T("var s = new DisposableStack(); var m = s.move(); R = Object.getPrototypeOf(m) === DisposableStack.prototype");
T("class X extends DisposableStack {} var s = new X(); var m = s.move(); R = [m instanceof X, Object.getPrototypeOf(m) === DisposableStack.prototype].join()");
T("var s = new DisposableStack(); s.move(); s.use({ [Symbol.dispose]() {} })");
T("var s = new DisposableStack(); s.move(); s.adopt(1, () => {})");
T("var s = new DisposableStack(); s.move(); s.defer(() => {})");
T("var s = new DisposableStack(); s.move(); R = String(s.dispose())");
T("var s = new DisposableStack(); s.move(); s.move()");

// ---- dispose: ordem LIFO, retorno, reentrada.
T("var log = []; var s = new DisposableStack(); s.defer(() => log.push(1)); s.defer(() => log.push(2)); s.defer(() => log.push(3)); R = String(s.dispose()) + ':' + log.join()");
T("var log = []; var s = new DisposableStack(); s.use({ [Symbol.dispose]() { log.push('u1') } }); s.adopt('a', v => log.push(v)); s.defer(() => log.push('d')); s.use({ [Symbol.dispose]() { log.push('u2') } }); s.dispose(); R = log.join()");
T("var s = new DisposableStack(); R = [s.disposed, String(s.dispose()), s.disposed, String(s.dispose())].join()");
T("var n = 0; var s = new DisposableStack(); s.defer(() => n++); s.dispose(); s.dispose(); s.dispose(); R = n");
T("var s = new DisposableStack(); var seen; s.defer(() => { seen = s.disposed }); s.dispose(); R = seen");
T("var s = new DisposableStack(); s.defer(() => s.defer(() => {})); s.dispose()");
T("var s = new DisposableStack(); s.defer(() => s.use(null)); s.dispose()");
T("var s = new DisposableStack(); s.defer(() => s.move()); s.dispose()");
T("var log = []; var s = new DisposableStack(); s.defer(() => { log.push('a'); s.dispose(); log.push('b') }); s.dispose(); R = log.join()");
T("var log = []; var o = { [Symbol.dispose]() { log.push(this === o) } }; var s = new DisposableStack(); s.use(o); s.dispose(); R = log.join()");
T("var log = []; var s = new DisposableStack(); var o = { get [Symbol.dispose]() { log.push('get'); return () => log.push('call') } }; s.use(o); log.push('used'); s.dispose(); R = log.join()");
T("var log = []; var s = new DisposableStack(); var o = { get [Symbol.dispose]() { log.push('get'); return () => log.push('call') } }; s.use(o); s.use(o); s.dispose(); R = log.join()");
T("var log = []; var s = new DisposableStack(); var o = { [Symbol.dispose]() { log.push('d1') } }; s.use(o); o[Symbol.dispose] = () => log.push('d2'); s.dispose(); R = log.join()");
T("var log = []; var s = new DisposableStack(); s.use({ [Symbol.dispose]() { return 5 } }); R = String(s.dispose())");
T("var s = new DisposableStack(); s.use({ [Symbol.dispose]: async function () { throw 1 } }); R = String(s.dispose())");
T("var s = new DisposableStack(); s.use({ [Symbol.dispose]() { return Promise.reject(1) } }); R = typeof s.dispose()");
T("var s = new DisposableStack(); s.use(Object.assign(function () {}, { [Symbol.dispose]() { R = 'fn' } })); s.dispose()");
T("var s = new DisposableStack(); s.use(null); s.use(undefined); R = String(s.dispose())");
T("var d = DisposableStack.prototype.dispose; R = [d.call(new DisposableStack()), typeof d].join()");
T("var d = DisposableStack.prototype[Symbol.dispose]; d.call({})");
T("var s = new DisposableStack(); var f = s.dispose; R = String(f.call(s)) + s.disposed");
T("var s = new DisposableStack(); var f = s.dispose; f()");

// ---- Erros suprimidos encadeados.
const shape = "R = e.constructor.name + '|' + e.name + '|' + e.message + '|' + Object.getOwnPropertyNames(e).sort().join() + '|' + (e.error === undefined ? 'u' : String(e.error)) + '|' + (e.suppressed === undefined ? 'u' : String(e.suppressed));";
T(`var s = new DisposableStack(); s.defer(() => { throw 'a' }); try { s.dispose() } catch (e) { R = String(e) + typeof e }`);
T(`var s = new DisposableStack(); var err = new RangeError('x'); s.defer(() => { throw err }); try { s.dispose() } catch (e) { R = String(e === err) }`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); s.defer(() => { throw 2 }); try { s.dispose() } catch (e) { ${shape} }`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); s.defer(() => { throw 2 }); s.defer(() => { throw 3 }); try { s.dispose() } catch (e) { R = [e.error, e.suppressed.error, e.suppressed.suppressed, e.suppressed instanceof SuppressedError, e instanceof SuppressedError].join() }`);
T(`var s = new DisposableStack(); for (var i = 1; i <= 5; i++) { var k = i; s.defer(() => { throw k }) } try { s.dispose() } catch (e) { var chain = []; while (e instanceof SuppressedError) { chain.push(e.error); e = e.suppressed } chain.push('end' + e); R = chain.join() }`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); s.defer(() => {}); s.defer(() => { throw 3 }); try { s.dispose() } catch (e) { ${shape} }`);
T(`var s = new DisposableStack(); s.defer(() => { throw undefined }); s.defer(() => { throw undefined }); try { s.dispose() } catch (e) { ${shape} }`);
T(`var s = new DisposableStack(); s.defer(() => { throw undefined }); try { s.dispose(); R = 'sem erro' } catch (e) { R = 'caught ' + e }`);
T(`var s = new DisposableStack(); s.defer(() => { throw new TypeError('t') }); s.defer(() => { throw new RangeError('r') }); try { s.dispose() } catch (e) { R = [e.error.name, e.suppressed.name, e.message, e.stack === undefined].join() }`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); s.defer(() => { throw 2 }); try { s.dispose() } catch (e) { R = [Object.getOwnPropertyDescriptor(e, 'error').enumerable, Object.getOwnPropertyDescriptor(e, 'suppressed').writable, Object.getOwnPropertyDescriptor(e, 'message') === undefined].join() }`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); s.defer(() => { throw 2 }); try { s.dispose() } catch (e) { R = String(e) + '|' + Object.prototype.toString.call(e) + '|' + e.toString === Error.prototype.toString }`);
T(`var log = []; var s = new DisposableStack(); s.defer(() => log.push('a')); s.defer(() => { throw 1 }); s.defer(() => log.push('c')); try { s.dispose() } catch (e) { log.push('caught ' + e) } R = log.join() + '|' + s.disposed`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); try { s.dispose() } catch (e) {} R = String(s.disposed) + ':' + String(s.dispose())`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); try { s.dispose() } catch (e) {} try { s.dispose(); R = 'second ok' } catch (e) { R = 'second ' + e }`);
T(`var s = new DisposableStack(); s.defer(() => { throw 1 }); try { s.dispose() } catch (e) {} s.use(null)`);
T(`var s = new DisposableStack(); s.use({ get [Symbol.dispose]() { throw 'getter' } })`);
T(`var s = new DisposableStack(); s.use({ [Symbol.dispose]: 1 })`);
T(`var s = new DisposableStack(); s.use({ [Symbol.dispose]: {} })`);
T(`var s = new DisposableStack(); s.use({ [Symbol.dispose]: 'x' })`);
T(`var s = new DisposableStack(); s.use(1)`);
T(`var s = new DisposableStack(); s.use('str')`);
T(`var s = new DisposableStack(); s.use({})`);
T(`var s = new DisposableStack(); s.use(Symbol())`);
T(`var s = new DisposableStack(); s.adopt(1, 2)`);
T(`var s = new DisposableStack(); s.defer(1)`);
T(`var s = new DisposableStack(); s.dispose(); s.use({ [Symbol.dispose]() {} })`);
T(`var s = new DisposableStack(); s.dispose(); s.use(null)`);
T(`var s = new DisposableStack(); s.dispose(); s.use(1)`);
T(`var s = new DisposableStack(); s.dispose(); s.adopt(1, () => {})`);
T(`var s = new DisposableStack(); s.dispose(); s.adopt(1, 2)`);
T(`var s = new DisposableStack(); s.dispose(); s.defer(() => {})`);
T(`var s = new DisposableStack(); s.dispose(); s.defer(1)`);
T(`var s = new DisposableStack(); s.dispose(); s.move()`);
// o erro de dispose preserva o objeto de erro original quando só um lança
T(`var s = new DisposableStack(); var e1 = { custom: 1 }; s.defer(() => { throw e1 }); try { s.dispose() } catch (e) { R = String(e === e1) + e.custom }`);

// ---- SuppressedError: construtor.
T("var e = new SuppressedError(1, 2, 'm'); R = [e.error, e.suppressed, e.message, e.name, Object.getOwnPropertyNames(e).sort().join()].join('|')");
T("var e = SuppressedError(1, 2, 'm'); R = [e.error, e.suppressed, e.message, e instanceof SuppressedError].join('|')");
T("var e = new SuppressedError(); R = [e.error, e.suppressed, e.message === '' , Object.getOwnPropertyNames(e).sort().join(), 'message' in e, e.hasOwnProperty('message')].join('|')");
T("var e = new SuppressedError(1); R = [e.error, e.suppressed, Object.getOwnPropertyNames(e).sort().join()].join('|')");
T("var e = new SuppressedError(1, 2); R = [e.error, e.suppressed, e.hasOwnProperty('message'), e.message === ''].join('|')");
T("var e = new SuppressedError(1, 2, undefined); R = [e.hasOwnProperty('message'), String(e.message)].join('|')");
T("var e = new SuppressedError(1, 2, null); R = [e.hasOwnProperty('message'), String(e.message)].join('|')");
T("var e = new SuppressedError(1, 2, { toString() { return 'obj' } }); R = e.message");
T("var e = new SuppressedError(1, 2, 12); R = typeof e.message + e.message");
T("var e = new SuppressedError(1, 2, Symbol())");
T("var e = new SuppressedError(1, 2, 'm', { cause: 'c' }); R = [e.hasOwnProperty('cause'), String(e.cause), Object.getOwnPropertyNames(e).sort().join()].join('|')");
T("var e = new SuppressedError(1, 2, 'm', {}); R = [e.hasOwnProperty('cause'), Object.getOwnPropertyNames(e).sort().join()].join('|')");
T("var e = new SuppressedError(1, 2, 'm', 'notobj'); R = [e.hasOwnProperty('cause'), Object.getOwnPropertyNames(e).join()].join('|')");
T(`var e = new SuppressedError(1, 2, 'm'); R = ${J}([Object.getOwnPropertyDescriptor(e, 'error'), Object.getOwnPropertyDescriptor(e, 'suppressed'), Object.getOwnPropertyDescriptor(e, 'message')])`);
T("var e = new SuppressedError(1, 2, 'm'); R = [Object.keys(e).join(), Object.prototype.hasOwnProperty.call(e, 'stack'), typeof e.stack].join('|')");
T("var e = new SuppressedError(1, 2, 'm'); R = String(e) + '|' + Object.prototype.toString.call(e) + '|' + (e instanceof Error) + '|' + (Object.getPrototypeOf(e) === SuppressedError.prototype)");
T("R = [SuppressedError.prototype.name, SuppressedError.prototype.message === '', SuppressedError.prototype.hasOwnProperty('error'), SuppressedError.prototype.hasOwnProperty('suppressed')].join('|')");
T(`R = [(${DESC})(SuppressedError.prototype, 'name'), (${DESC})(SuppressedError.prototype, 'message')].join('|')`);
T("R = Error.prototype.toString.call(new SuppressedError(1, 2, 'boom'))");
T("var e = new SuppressedError(1, 2, 'boom'); e.name = 'X'; R = String(e)");
T("R = SuppressedError.prototype.toString === Error.prototype.toString");
T("class E2 extends SuppressedError { constructor() { super('a', 'b', 'c') } } var e = new E2(); R = [e.name, e.error, e.suppressed, e.message, e instanceof SuppressedError, e.constructor.name].join('|')");
T("var e = Reflect.construct(SuppressedError, [1, 2, 'm'], Object); R = [Object.getPrototypeOf(e) === Object.prototype, e.error].join('|')");
T("var o = Reflect.construct(SuppressedError, [1, 2, 'm'], Array); R = [Array.isArray(o), o instanceof Array, o.message].join('|')");
T("R = Object.prototype.toString.call(SuppressedError.prototype)");
T("R = String(Error.captureStackTrace)");
T("var e = new SuppressedError(new Error('inner'), new RangeError('sup'), 'combined'); R = [e.error.message, e.suppressed.name, e.message].join('|')");
T("var e = new SuppressedError(); e.error = 5; e.suppressed = 6; R = e.error + e.suppressed");
T("var e = new SuppressedError(undefined, undefined, undefined); R = Object.getOwnPropertyNames(e).sort().join()");
T(`var e = new SuppressedError(1, 2, 'm'); R = ${J}(e)`);
T("var e = new SuppressedError(1, 2, 'm'); R = Object.getOwnPropertyNames(e).join()");
T("R = [new SuppressedError(1, 2, 'm').stack.split('\\n')[0]].join()");
T("var e = new SuppressedError(1, 2, 'm'); R = Error.prototype.isPrototypeOf(e) + ':' + SuppressedError.prototype.isPrototypeOf(e)");
T("var proto = Object.getPrototypeOf(SuppressedError); R = (proto === Error) + ':' + proto.name");
T("var e = new SuppressedError(1, 2, 'm'); R = Object.getOwnPropertyNames(Object.getPrototypeOf(e)).sort().join()");
T("var order = []; new SuppressedError({ get x() { order.push('e') } }.x, 0, { toString() { order.push('m'); return '' } }); R = order.join()");
T("var order = []; var o = { get cause() { order.push('cause'); return 1 }, }; new SuppressedError(1, 2, { toString() { order.push('msg'); return 'x' } }, o); R = order.join()");

// ---- Programas assíncronos: AsyncDisposableStack.
T("var s = new AsyncDisposableStack(); var p = s.disposeAsync(); R = [p instanceof Promise, s.disposed, Object.getPrototypeOf(p) === Promise.prototype].join()");
T("var p = AsyncDisposableStack.prototype.disposeAsync.call({}); R = [p instanceof Promise, Object.getPrototypeOf(p) === Promise.prototype].join(); p.catch(() => {})");
A("var s = new AsyncDisposableStack(); R = String(await s.disposeAsync())");
A("var s = new AsyncDisposableStack(); await s.disposeAsync(); R = String(await s.disposeAsync()) + s.disposed");
A("var s = new AsyncDisposableStack(); await AsyncDisposableStack.prototype.disposeAsync.call({}).catch(e => { R = e.name + ': ' + e.message })");
A("var s = new AsyncDisposableStack(); await AsyncDisposableStack.prototype.disposeAsync.call(new DisposableStack()).catch(e => { R = e.name + ': ' + e.message })");
A("var s = new AsyncDisposableStack(); await AsyncDisposableStack.prototype.disposeAsync.call(undefined).catch(e => { R = e.name + ': ' + e.message })");
A("var log = []; var s = new AsyncDisposableStack(); s.defer(async () => { await 0; log.push(1) }); s.defer(async () => { await 0; log.push(2) }); s.defer(() => log.push(3)); await s.disposeAsync(); R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { log.push('async'); return Promise.resolve() } }); s.use({ [Symbol.dispose]() { log.push('sync') } }); await s.disposeAsync(); R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { log.push('async') }, [Symbol.dispose]() { log.push('sync') } }); await s.disposeAsync(); R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]: undefined, [Symbol.dispose]() { log.push('sync') } }); await s.disposeAsync(); R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]: null, [Symbol.dispose]() { log.push('sync') } }); await s.disposeAsync(); R = log.join()");
A("var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]: 1, [Symbol.dispose]() {} })");
A("var s = new AsyncDisposableStack(); s.use({ [Symbol.dispose]: 1 })");
A("var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]: {} })");
A("var s = new AsyncDisposableStack(); s.use({})");
A("var s = new AsyncDisposableStack(); s.use(1)");
A("var s = new AsyncDisposableStack(); s.use('x')");
A("var s = new AsyncDisposableStack(); R = String(s.use(null)) + String(s.use(undefined))");
A("var s = new AsyncDisposableStack(); s.defer(1)");
A("var s = new AsyncDisposableStack(); s.adopt(1, 2)");
A("var s = new AsyncDisposableStack(); s.adopt(1)");
A("var s = new AsyncDisposableStack(); R = String(s.adopt(5, () => {})) + String(s.defer(() => {}))");
A("var log = []; var s = new AsyncDisposableStack(); s.adopt('v', async v => { await 0; log.push(v) }); await s.disposeAsync(); R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.defer(async function () { log.push([arguments.length, this === undefined ? 'u' : typeof this].join(':')) }); await s.disposeAsync(); R = log.join()");
A("var log = []; var o = { [Symbol.asyncDispose]() { log.push(this === o) } }; var s = new AsyncDisposableStack(); s.use(o); await s.disposeAsync(); R = log.join()");
A("var s = new AsyncDisposableStack(); s.defer(() => { throw 'a' }); await s.disposeAsync().catch(e => { R = String(e) + typeof e })");
A("var s = new AsyncDisposableStack(); s.defer(async () => { throw 1 }); s.defer(async () => { throw 2 }); await s.disposeAsync().catch(e => { " + shape + " })");
A("var s = new AsyncDisposableStack(); s.defer(() => { throw 1 }); s.defer(() => { throw 2 }); s.defer(() => { throw 3 }); await s.disposeAsync().catch(e => { R = [e.error, e.suppressed.error, e.suppressed.suppressed].join() })");
A("var s = new AsyncDisposableStack(); s.defer(() => Promise.reject(1)); s.defer(() => Promise.reject(2)); await s.disposeAsync().catch(e => { R = [e instanceof SuppressedError, e.error, e.suppressed].join() })");
A("var s = new AsyncDisposableStack(); s.defer(() => { throw 1 }); s.defer(() => {}); await s.disposeAsync().catch(e => { R = String(e) })");
A("var s = new AsyncDisposableStack(); s.defer(() => { throw 1 }); await s.disposeAsync().catch(() => {}); R = String(s.disposed) + String(await s.disposeAsync())");
A("var log = []; var s = new AsyncDisposableStack(); s.defer(() => log.push('a')); s.defer(async () => { await 0; throw 'x' }); s.defer(() => log.push('c')); await s.disposeAsync().catch(e => log.push('caught ' + e)); R = log.join()");
A("var s = new AsyncDisposableStack(); s.disposeAsync(); s.use(null)");
A("var s = new AsyncDisposableStack(); s.disposeAsync(); s.adopt(1, () => {})");
A("var s = new AsyncDisposableStack(); s.disposeAsync(); s.defer(() => {})");
A("var s = new AsyncDisposableStack(); s.disposeAsync(); s.move()");
A("var s = new AsyncDisposableStack(); var log = []; s.defer(() => log.push('x')); var m = s.move(); R = [s.disposed, m.disposed, m instanceof AsyncDisposableStack].join(); await m.disposeAsync(); R += '|' + log.join()");
A("var s = new AsyncDisposableStack(); s.disposeAsync(); R = s.disposed");
A("var seen; var s = new AsyncDisposableStack(); s.defer(() => { seen = s.disposed }); var p = s.disposeAsync(); R0 = [s.disposed, seen]; await p; R = [s.disposed, seen].join()");
A("var log = []; var s = new AsyncDisposableStack(); s.defer(() => log.push('first')); var p = s.disposeAsync(); log.push('after call'); await p; R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.defer(() => log.push('sync body')); s.defer(async () => { log.push('a1'); await 0; log.push('a2') }); await s.disposeAsync(); R = log.join()");
A("var log = []; var s = new AsyncDisposableStack(); s.defer(() => log.push('x')); Promise.resolve().then(() => log.push('tick1')).then(() => log.push('tick2')).then(() => log.push('tick3')); await s.disposeAsync(); log.push('done'); R = log.join()");
A("var s = new AsyncDisposableStack(); var p1 = s.disposeAsync(), p2 = s.disposeAsync(); R = String(p1 === p2)");
A("var s = new DisposableStack(); var m = new AsyncDisposableStack(); R = [s.disposed, m.disposed].join()");
A("var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { return { then(r) { r() } } } }); R = String(await s.disposeAsync())");
A("var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { return { then(_, rej) { rej('thenable') } } } }); await s.disposeAsync().catch(e => { R = String(e) })");
A("var s = new AsyncDisposableStack(); s.use({ [Symbol.asyncDispose]() { return 42 } }); R = String(await s.disposeAsync())");
A("var s = new AsyncDisposableStack(); s.use({ get [Symbol.asyncDispose]() { throw 'g' } })");
A("var s = new AsyncDisposableStack(); s.use({ get [Symbol.asyncDispose]() { return undefined }, get [Symbol.dispose]() { throw 'sg' } })");
A("var s = new AsyncDisposableStack(); R = typeof s.use({ [Symbol.asyncDispose]() {} })");
A("var s = new AsyncDisposableStack(); var o = { [Symbol.asyncDispose]() {} }; R = String(s.use(o) === o)");
A("var d = AsyncDisposableStack.prototype[Symbol.asyncDispose]; R = String(d === AsyncDisposableStack.prototype.disposeAsync)");
// sync dispose de um valor com apenas asyncDispose
T("var s = new DisposableStack(); s.use({ [Symbol.asyncDispose]() {} })");

// ---- Sintaxe `using` e `await using`.
T("var log = []; { using a = { [Symbol.dispose]() { log.push('a') } }; using b = { [Symbol.dispose]() { log.push('b') } }; log.push('body') } R = log.join()");
T("var log = []; { using a = null; using b = undefined; log.push('ok') } R = log.join()");
T("{ using a = 1 }");
T("{ using a = 'x' }");
T("{ using a = {} }");
T("{ using a = Symbol() }");
T("{ using a = { [Symbol.dispose]: 1 } }");
T("{ using a = { [Symbol.asyncDispose]() {} } }");
T("{ using a = { get [Symbol.dispose]() { throw 'g' } } }");
T("var log = []; try { { using a = { [Symbol.dispose]() { log.push('a'); throw 'ea' } }; throw 'body' } } catch (e) { R = [log.join(), e.constructor.name, e.error, e.suppressed].join('|') }");
T("var log = []; try { { using a = { [Symbol.dispose]() { throw 'ea' } }; using b = { [Symbol.dispose]() { throw 'eb' } }; } } catch (e) { R = [e.constructor.name, e.error, e.suppressed].join('|') }");
T("try { { using a = { [Symbol.dispose]() { throw 'ea' } }; using b = { [Symbol.dispose]() { throw 'eb' } }; using c = { [Symbol.dispose]() { throw 'ec' } }; throw 'body' } } catch (e) { var c = []; while (e instanceof SuppressedError) { c.push(e.error); e = e.suppressed } c.push(e); R = c.join() }");
T("try { { using a = { [Symbol.dispose]() { throw 'ea' } }; } } catch (e) { R = typeof e + e }");
T("var log = []; (function () { using a = { [Symbol.dispose]() { log.push('disposed') } }; log.push('body'); return log.push('ret') })(); R = log.join()");
T("var log = []; function f() { using a = { [Symbol.dispose]() { log.push('d') } }; return (log.push('expr'), 'value') } R = f() + '|' + log.join()");
T("var log = []; for (var i = 0; i < 3; i++) { using a = { [Symbol.dispose]() { log.push('d' + i) } }; if (i == 1) continue; log.push('b' + i) } R = log.join()");
T("var log = []; for (var i = 0; i < 3; i++) { using a = { [Symbol.dispose]() { log.push('d' + i) } }; if (i == 1) break } R = log.join()");
T("var log = []; for (using a of [{ [Symbol.dispose]() { log.push('d1') } }, { [Symbol.dispose]() { log.push('d2') } }]) { log.push('body') } R = log.join()");
T("var log = []; for (using a of [null, undefined]) { log.push('b') } R = log.join()");
T("for (using a of [1]) {}");
T("var log = []; for (using a = { [Symbol.dispose]() { log.push('d') } }; log.length < 1; ) { log.push('b') } R = log.join()");
T("var log = []; switch (1) { case 1: using a = { [Symbol.dispose]() { log.push('d') } }; log.push('case'); } R = log.join()");
T("var log = []; label: { using a = { [Symbol.dispose]() { log.push('d') } }; break label; } R = log.join()");
T("var log = []; try { using a = { [Symbol.dispose]() { log.push('d') } }; throw 1 } catch (e) { log.push('c' + e) } finally { log.push('f') } R = log.join()");
T("var log = []; function* g() { using a = { [Symbol.dispose]() { log.push('d') } }; yield 1; yield 2 } var it = g(); it.next(); it.return(9); R = log.join()");
T("var log = []; function* g() { using a = { [Symbol.dispose]() { log.push('d') } }; yield 1; yield 2 } var it = g(); it.next(); R = log.join() + '|' + it.next().value + '|' + it.next().done + '|' + log.join()");
T("var log = []; function* g() { using a = { [Symbol.dispose]() { log.push('d') } }; yield 1 } for (var v of g()) { break } R = log.join()");
T("var log = []; var f = () => { using a = { [Symbol.dispose]() { log.push('d') } }; return 1 }; R = f() + log.join()");
T("var log = []; class C { m() { using a = { [Symbol.dispose]() { log.push('d') } }; return 1 } } R = new C().m() + log.join()");
T("var log = []; { using a = { [Symbol.dispose]() { log.push(this === a) } } } R = log.join()");
T("var log = []; { var o = { get [Symbol.dispose]() { log.push('get'); return () => log.push('call') } }; using a = o; log.push('body') } R = log.join()");
T("var log = []; { using a = { [Symbol.dispose]() { log.push('d') } }; a = 1 }");
T("var a = 1; { using a = { [Symbol.dispose]() {} }; R = typeof a }");
T("{ using a = null; R = String(a) }");
T("{ using a = { [Symbol.dispose]() {} }, b = { [Symbol.dispose]() {} }; R = typeof a + typeof b }");
T("var log = []; { using a = { [Symbol.dispose]() { log.push('a') } }, b = { [Symbol.dispose]() { log.push('b') } }; } R = log.join()");
T("var log = []; { using a = { [Symbol.dispose]() { log.push('a') } }, b = 1; } R = log.join()");
T("var log = []; try { { using a = { [Symbol.dispose]() { log.push('a') } }, b = 1; } } catch (e) { log.push(e.name) } R = log.join()");
T("var log = []; { using a = { [Symbol.dispose]() { log.push('a'); } }; { using b = { [Symbol.dispose]() { log.push('b') } }; } log.push('mid') } R = log.join()");
T("var log = []; try { { using a = { [Symbol.dispose]() { log.push('a') } }; null.x } } catch (e) { log.push(e.name) } R = log.join()");
T("var log = []; { using using = null; log.push(typeof using) } R = log.join()");
T("var using = 1; var x = using\nusing\nR = [x, using].join()");
T("var using = { of: 1 }; var r = []; for (using of [1]) r.push(using); R = r.join()");
T("var o = { using: 5 }; R = o.using");
T("var using; using = 2; R = using");
T("var await = 3; R = await");
T("function f(using) { return using } R = f(4)");
T("var using = [1]; R = using[0]");
T("var using = (x) => x; R = using(8)");
for (const src of [
  "using x = null;", "{ using x; }", "{ using [a] = null; }", "{ using {a} = null; }", "{ using x = null, y; }", "for (using x in {}) ;",
  "for (using x = null, y of []) ;", "switch (1) { case 1: using x = null; }", "{ using\nx = null; }", "{ using x = null; using x = null; }",
  "{ let x; using x = null; }", "{ using x = null; var x; }", "function f() { using x = null }", "label: using x = null;", "if (1) using x = null;",
  "export using x = null;", "{ using let = null; }", "{ using yield = null; }", "{ using await = null; }", "async function f() { using await = null }",
  "{ using of = null; }", "for (using of of []) ;", "for (using of = null; false;) ;", "for (using x of []) ;", "{ await using x = null; }",
  "{ using x = null\n using y = null }", "class C { static { using x = null } }", "{ using x = 1, = 2 }", "{ const using = 1; }", "for (using\nx of []) ;",
  "function f() { { using x = null; } }", "{ async using x = null }", "var f = () => { using x = null }", "{ using x = null } using y = null;",
]) S(src);
for (const src of [
  "async function f() { await using x = null }", "async function f() { { await using x = null; } }", "async function f() { for (await using x of []) ; }",
  "async function f() { for await (await using x of []) ; }", "async function f() { await using\nx = null }", "async function f() { await using [a] = null }",
  "async function f() { await using x }", "function f() { await using x = null }", "async function f() { await using x = null, y = null }",
  "async function f() { switch (1) { case 1: await using x = null } }", "async () => { await using x = null }", "async function* f() { await using x = null }",
  "async function f() { for (await using x = null; false;) ; }", "async function f() { label: await using x = null; }", "async function f() { { await using using = null } }",
]) S(src);
A("var log = []; { await using a = { async [Symbol.asyncDispose]() { await 0; log.push('a') } }; await using b = { [Symbol.asyncDispose]() { log.push('b') } }; log.push('body') } R = log.join()");
A("var log = []; { await using a = { [Symbol.dispose]() { log.push('sync') } }; } R = log.join()");
A("var log = []; { await using a = { [Symbol.asyncDispose]() { log.push('async') }, [Symbol.dispose]() { log.push('sync') } }; } R = log.join()");
A("var log = []; { await using a = null; await using b = undefined; } R = 'null ok'");
A("{ await using a = 1 }");
A("{ await using a = {} }");
A("{ await using a = { [Symbol.asyncDispose]: 1 } }");
A("{ await using a = { [Symbol.dispose]: 1 } }");
A("{ await using a = { [Symbol.asyncDispose]: null, [Symbol.dispose]: null } }");
A("var log = []; try { { await using a = { async [Symbol.asyncDispose]() { throw 'ea' } }; throw 'body' } } catch (e) { R = [e.constructor.name, e.error, e.suppressed].join('|') }");
A("try { { await using a = { async [Symbol.asyncDispose]() { throw 'ea' } }; await using b = { [Symbol.asyncDispose]() { return Promise.reject('eb') } }; } } catch (e) { R = [e.constructor.name, e.error, e.suppressed].join('|') }");
A("try { { using a = { [Symbol.dispose]() { throw 'sa' } }; await using b = { async [Symbol.asyncDispose]() { throw 'ab' } }; } } catch (e) { R = [e.constructor.name, e.error, e.suppressed].join('|') }");
A("var log = []; for (await using a of [{ async [Symbol.asyncDispose]() { log.push('d1') } }, { [Symbol.asyncDispose]() { log.push('d2') } }]) { log.push('body') } R = log.join()");
A("var log = []; async function f() { await using a = { [Symbol.asyncDispose]() { log.push('d') } }; log.push('body'); return 'ret' } var r = await f(); R = r + '|' + log.join()");
A("var log = []; async function f() { await using a = { [Symbol.asyncDispose]() { log.push('d') } }; return (log.push('expr'), 'v') } var p = f(); log.push('called'); await p; R = log.join()");
A("var log = []; async function f() { await using a = { [Symbol.asyncDispose]() { return new Promise(r => { log.push('pending'); r() }) } }; log.push('body') } var p = f(); log.push('after'); await p; R = log.join()");
A("var log = []; async function* g() { await using a = { [Symbol.asyncDispose]() { log.push('d') } }; yield 1; yield 2 } var it = g(); await it.next(); await it.return(5); R = log.join()");
A("var log = []; async function* g() { await using a = { [Symbol.asyncDispose]() { log.push('d') } }; yield 1; yield 2 } for await (var v of g()) { break } R = log.join()");
A("var log = []; { await using a = { [Symbol.asyncDispose]() { log.push('d') } }; log.push('body'); } Promise.resolve().then(() => log.push('micro')); R = log.join()");
A("var log = []; { await using a = null; log.push('b') } R = log.join()");
A("var log = []; { using a = { [Symbol.dispose]() { log.push('sync') } }; await using b = { [Symbol.asyncDispose]() { log.push('async') } }; } R = log.join()");
A("var log = []; { await using a = { [Symbol.asyncDispose]() { log.push('async') } }; using b = { [Symbol.dispose]() { log.push('sync') } }; } R = log.join()");
A("var log = []; { var t = Promise.resolve().then(() => log.push('tick')); await using a = { [Symbol.asyncDispose]() { log.push('d') } }; } await t; R = log.join()");
A("var log = []; { await using a = null; log.push('after null') } await 0; R = log.join()");
A("var log = []; await Promise.resolve(); { using a = { [Symbol.dispose]() { log.push('d') } }; await 0; log.push('body') } R = log.join()");

// ---- Protótipos de iteradores e Symbol.iterator.
T(`R = (${DESC})(Iterator.prototype, Symbol.dispose)`);
T("R = [typeof Iterator.prototype[Symbol.dispose], Iterator.prototype[Symbol.dispose].name, Iterator.prototype[Symbol.dispose].length].join()");
T("var f = Iterator.prototype[Symbol.dispose]; R = [f.hasOwnProperty('prototype'), Object.getPrototypeOf(f) === Function.prototype].join()");
T("var log = []; var it = { return() { log.push('ret'); return {} } }; Object.setPrototypeOf(it, Iterator.prototype); R = String(it[Symbol.dispose]()) + log.join()");
T("var it = Object.create(Iterator.prototype); R = String(it[Symbol.dispose]())");
T("var it = Object.create(Iterator.prototype); it.return = 1; it[Symbol.dispose]()");
T("var it = Object.create(Iterator.prototype); it.return = null; R = String(it[Symbol.dispose]())");
T("var it = Object.create(Iterator.prototype); it.return = () => { throw 'r' }; it[Symbol.dispose]()");
T("var it = Object.create(Iterator.prototype); it.return = () => 5; R = String(it[Symbol.dispose]())");
T("var log = []; var it = Object.create(Iterator.prototype); it.return = function () { log.push(this === it, arguments.length) }; it[Symbol.dispose](); R = log.join()");
T("Iterator.prototype[Symbol.dispose].call(1)");
T("Iterator.prototype[Symbol.dispose].call(undefined)");
T("Iterator.prototype[Symbol.dispose].call(null)");
T("R = String(Iterator.prototype[Symbol.dispose].call({}))");
T("R = String(Iterator.prototype[Symbol.dispose].call({ return: undefined }))");
T("var log = []; Iterator.prototype[Symbol.dispose].call({ return() { log.push('ok') } }); R = log.join()");
T("var it = [1, 2, 3][Symbol.iterator](); R = typeof it[Symbol.dispose] + ':' + String(it[Symbol.dispose]())");
T("var it = [1, 2, 3][Symbol.iterator](); it[Symbol.dispose](); R = JSON.stringify(it.next())");
T("var it = [1, 2, 3].values(); it.next(); R = it.hasOwnProperty(Symbol.dispose) + ':' + (Symbol.dispose in it)");
T("function* g() { try { yield 1; yield 2 } finally { R = 'fin' } } var it = g(); it.next(); it[Symbol.dispose](); R += '|' + JSON.stringify(it.next())");
T("function* g() { yield 1 } var it = g(); it[Symbol.dispose](); R = JSON.stringify(it.next())");
T("function* g() { yield 1 } R = typeof g()[Symbol.dispose] + ':' + (g()[Symbol.dispose] === Iterator.prototype[Symbol.dispose])");
T("var it = new Map([[1, 2]])[Symbol.iterator](); R = (it[Symbol.dispose] === Iterator.prototype[Symbol.dispose]) + ':' + String(it[Symbol.dispose]())");
T("var it = 'ab'[Symbol.iterator](); R = (it[Symbol.dispose] === Iterator.prototype[Symbol.dispose]) + ':' + String(it[Symbol.dispose]())");
T("var it = 'a'.matchAll(/a/g); R = (it[Symbol.dispose] === Iterator.prototype[Symbol.dispose]) + ''");
T("var it = Iterator.from({ next() { return { done: true } } }); R = typeof it[Symbol.dispose] + ':' + String(it[Symbol.dispose]())");
T("var log = []; var it = Iterator.from({ next() { return { done: false, value: 1 } }, return() { log.push('ret'); return {} } }); it[Symbol.dispose](); R = log.join()");
T("var log = []; var it = [1, 2, 3].values().map(x => x); it[Symbol.dispose](); R = JSON.stringify(it.next())");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; var it = src.map(x => x); it[Symbol.dispose](); R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; var it = src.filter(x => x); it.next(); it[Symbol.dispose](); R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; var it = src.take(1); it.next(); it[Symbol.dispose](); R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; var it = src.drop(1); it[Symbol.dispose](); R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; var it = src.flatMap(x => [x]); it[Symbol.dispose](); R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; src.some(x => true); R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; { using it = src.map(x => x); it.next(); } R = log.join()");
T("var log = []; var src = { next() { return { done: false, value: 1 } }, return() { log.push('src ret'); return {} }, __proto__: Iterator.prototype }; { using it = src; } R = log.join()");
T("var log = []; { using it = { __proto__: Iterator.prototype, next() { return { done: true } }, return() { log.push('return called') } }; } R = log.join()");
T("R = Object.getOwnPropertySymbols(Iterator.prototype).map(String).sort().join()");
T("R = Reflect.ownKeys(Iterator.prototype).map(String).join()");
T("R = Object.getOwnPropertyNames(Iterator.prototype).sort().join()");
T(`R = (${DESC})(Iterator.prototype, Symbol.toStringTag) + '|' + (${DESC})(Iterator.prototype, 'constructor')`);
T("var d = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag); R = [typeof d.get, typeof d.set, d.get.name, d.set.name].join()");
T("R = [Iterator.prototype[Symbol.toStringTag], Object.prototype.toString.call(Iterator.prototype)].join()");
T("var d = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor'); R = [typeof d.get, typeof d.set, d.get.name, d.set.name, d.enumerable, d.configurable].join()");
T("R = [Iterator.prototype[Symbol.iterator].name, Iterator.prototype[Symbol.iterator].length, (" + DESC + ")(Iterator.prototype, Symbol.iterator)].join()");
T("R = String(Iterator.prototype[Symbol.iterator].call(5))");
T("var o = {}; R = String(Iterator.prototype[Symbol.iterator].call(o) === o)");
T("R = [Object.getPrototypeOf(Iterator.prototype) === Object.prototype, Object.getPrototypeOf(Iterator) === Function.prototype, Iterator.name, Iterator.length].join()");
T(`R = [(${DESC})(Iterator, 'prototype'), (${DESC})(globalThis, 'Iterator')].join()`);
T("Iterator()");
T("new Iterator()");
T("class I extends Iterator {} R = typeof new I()[Symbol.dispose]");
T("R = Object.getOwnPropertyNames(Iterator).sort().join()");

// Descritores de Symbol.iterator das coleções e dos arrays tipados.
const iterOwners = [
  ["Array.prototype", "values"], ["Map.prototype", "entries"], ["Set.prototype", "values"], ["String.prototype", null],
  ["Int8Array.prototype", "values"], ["Uint8Array.prototype", "values"], ["Float64Array.prototype", "values"], ["BigInt64Array.prototype", "values"],
  ["Object.getPrototypeOf(Int8Array).prototype", "values"],
];
for (const [owner, alias] of iterOwners) {
  T(`R = (${DESC})(${owner}, Symbol.iterator)`);
  T(`var f = ${owner}[Symbol.iterator]; R = [typeof f, f.name, f.length, f.hasOwnProperty('prototype')].join()`);
  if (alias) {
    T(`R = String(${owner}[Symbol.iterator] === ${owner}.${alias})`);
    T(`R = [${owner}.${alias}.name, ${owner}.${alias}.length, (${DESC})(${owner}, '${alias}')].join()`);
  }
  T(`${owner}[Symbol.iterator].call(undefined)`);
  T(`${owner}[Symbol.iterator].call(null)`);
  T(`R = typeof ${owner}[Symbol.iterator].call(${owner === "String.prototype" ? "'ab'" : owner.startsWith("Map") ? "new Map()" : owner.startsWith("Set") ? "new Set()" : owner.includes("Int8") || owner.includes("Uint8") || owner.includes("Float64") || owner.includes("BigInt") || owner.includes("getPrototypeOf") ? "new Uint8Array(1)" : "[]"}).next`);
}
T("Map.prototype[Symbol.iterator].call({})");
T("Map.prototype[Symbol.iterator].call(new Set())");
T("Set.prototype[Symbol.iterator].call(new Map())");
T("Set.prototype[Symbol.iterator].call({})");
T("Int8Array.prototype[Symbol.iterator].call([])");
T("Uint8Array.prototype.values.call({})");
T("var it = Array.prototype[Symbol.iterator].call({ length: 2, 0: 'a', 1: 'b' }); R = JSON.stringify([it.next(), it.next(), it.next()])");
T("var it = Array.prototype[Symbol.iterator].call('xy'); R = JSON.stringify([it.next(), it.next(), it.next()])");
T("var it = Array.prototype[Symbol.iterator].call(5); R = JSON.stringify(it.next())");
T("var it = String.prototype[Symbol.iterator].call(5); R = JSON.stringify(it.next())");
T("var it = String.prototype[Symbol.iterator].call({ toString() { return 'q' } }); R = JSON.stringify(it.next())");
T("R = String(String.prototype[Symbol.iterator].call(Symbol()))");
T("R = JSON.stringify([...'a\\ud83d\\ude00b'])");
T("R = JSON.stringify([...'\\ud83d'].map(c => c.length))");
// Protótipos dos iteradores nativos.
const iterKinds = [
  ["ArrayIterator", "[][Symbol.iterator]()"], ["MapIterator", "new Map()[Symbol.iterator]()"], ["SetIterator", "new Set()[Symbol.iterator]()"],
  ["StringIterator", "''[Symbol.iterator]()"], ["RegExpStringIterator", "'a'.matchAll(/a/g)"], ["IteratorHelper", "[].values().map(x => x)"],
  ["WrapForValid", "Iterator.from({ next() {} })"], ["Generator", "(function* () {})()"],
];
for (const [name, expr] of iterKinds) {
  T(`var p = Object.getPrototypeOf(${expr}); R = [Object.prototype.toString.call(${expr}), p[Symbol.toStringTag], Object.getOwnPropertyNames(p).sort().join(), Object.getOwnPropertySymbols(p).map(String).join()].join('|')`);
  T(`var p = Object.getPrototypeOf(${expr}); R = (Object.getPrototypeOf(p) === Iterator.prototype) + ':' + (Object.getPrototypeOf(p) === Object.prototype)`);
  T(`var p = Object.getPrototypeOf(${expr}); R = [(${DESC})(p, Symbol.toStringTag), (${DESC})(p, 'next'), (${DESC})(p, 'return'), (${DESC})(p, 'throw')].join('|')`);
  T(`var p = Object.getPrototypeOf(${expr}); R = [typeof p.next, p.next && p.next.name, p.next && p.next.length].join()`);
}
T("var p = Object.getPrototypeOf(Object.getPrototypeOf((function* () {})())); R = [Object.prototype.toString.call(p), typeof p.next, (p === Iterator.prototype), Object.getOwnPropertyNames(p).sort().join()].join('|')");
T("var GP = Object.getPrototypeOf(function* () {}).prototype; R = [GP[Symbol.toStringTag], GP.next.name, GP.return.name, GP.throw.name, GP.next.length, GP.return.length, GP.throw.length, (GP === Object.getPrototypeOf(Object.getPrototypeOf((function* () {})())))].join()");
T("var AGP = Object.getPrototypeOf(async function* () {}).prototype; R = [AGP[Symbol.toStringTag], AGP.next.name, AGP.return.name, AGP.throw.name, Object.getOwnPropertyNames(AGP).sort().join()].join('|')");
T("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = [Object.getOwnPropertySymbols(AIP).map(String).join(), AIP[Symbol.asyncIterator].name, AIP[Symbol.asyncIterator].length, typeof AIP[Symbol.asyncDispose], Object.getOwnPropertyNames(AIP).length].join('|')");
T(`var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = [(${DESC})(AIP, Symbol.asyncDispose), (${DESC})(AIP, Symbol.asyncIterator)].join('|')`);
T("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = [AIP[Symbol.asyncDispose].name, AIP[Symbol.asyncDispose].length].join()");
T("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = String(Object.getPrototypeOf(AIP) === Object.prototype)");
T("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = String(AIP[Symbol.asyncDispose].call({ return() { return 1 } }) instanceof Promise)");
T("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); var p = AIP[Symbol.asyncDispose].call(1); R = p instanceof Promise; p.catch(() => {})");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); try { await AIP[Symbol.asyncDispose].call(1) } catch (e) { R = e.name + ': ' + e.message }");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); try { await AIP[Symbol.asyncDispose].call(undefined) } catch (e) { R = e.name + ': ' + e.message }");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = String(await AIP[Symbol.asyncDispose].call({}))");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); R = String(await AIP[Symbol.asyncDispose].call({ return: null }))");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); var log = []; await AIP[Symbol.asyncDispose].call({ return(...a) { log.push(a.length); return Promise.resolve(5) } }); R = log.join()");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); try { await AIP[Symbol.asyncDispose].call({ return() { throw 'bad' } }) } catch (e) { R = String(e) }");
A("var AIP = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype); try { await AIP[Symbol.asyncDispose].call({ return() { return Promise.reject('rej') } }) } catch (e) { R = String(e) }");
A("async function* g() { try { yield 1; yield 2 } finally { R = 'afin' } } var it = g(); await it.next(); await it[Symbol.asyncDispose](); R += '|' + JSON.stringify(await it.next())");
A("async function* g() { yield 1 } var it = g(); R = [typeof it[Symbol.asyncDispose], it[Symbol.asyncDispose] === Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(it)))[Symbol.asyncDispose]].join()");
A("async function* g() { yield 1 } var it = g(); await it[Symbol.asyncDispose](); R = JSON.stringify(await it.next())");
A("async function* g() { yield 1 } { await using it = g(); await it.next(); } R = 'ok'");
A("var log = []; async function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } { await using it = g(); await it.next(); log.push('body') } R = log.join()");
A("var log = []; function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } { using it = g(); it.next(); log.push('body') } R = log.join()");
A("var log = []; function* g() { try { yield 1; yield 2 } finally { log.push('fin') } } { await using it = g(); it.next(); log.push('body') } R = log.join()");
// Iterator.prototype[Symbol.dispose] com iteradores de Array/Map/Set na sintaxe using.
T("{ using it = [1, 2][Symbol.iterator](); R = JSON.stringify(it.next()) }");
T("{ using it = new Map([[1, 2]]).entries(); R = JSON.stringify(it.next()) }");
T("{ using it = new Set([1]).values(); R = JSON.stringify(it.next()) }");
T("{ using it = 'ab'[Symbol.iterator](); R = JSON.stringify(it.next()) }");
T("{ using it = [1, 2, 3].values().map(x => x * 2); R = JSON.stringify(it.toArray()) }");
T("var it; { using i = [1, 2, 3].values(); it = i } R = JSON.stringify(it.next())");
T("var it; { using i = [1, 2, 3].values().map(x => x); it = i } R = JSON.stringify(it.next())");
T("var it; { using i = (function* () { yield 1 })(); it = i } R = JSON.stringify(it.next())");
T("var it; { using i = new Map([[1, 2]]).values(); it = i } R = JSON.stringify(it.next())");

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "dispose-golden-"));
// O programa vai por `vm.runInThisContext` (ProgramExecutable do JSC puro, sem o transpilador do bun). O SyntaxError de
// compilação é engolido e `R` fica indefinido ("<undefined>"). O hook de saída lê `R` depois de esvaziada a fila.
const source_file = path.join(dir, "dispose_source.js");
const file = path.join(dir, "dispose_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ").replace(/\bR0 = /g, "var R0 = ").replace(/\bR \+= /g, "globalThis.R += ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
