// Gera tests/golden/process_bun.tsv: o global `process` medido no bun 1.4.2, catalogado como os demais em
// scripts/golden-prelude.js (`JSON(sufixo)<TAB>JSON(resultado)<TAB>índice`, prelúdio em process.preludes.json).
// Cada programa roda como `main.js` (ou `main.mjs`, se o fonte traz o marcador `/*mjs*/`) num processo bun filho
// próprio, no diretório temporário normalizado para /app (como scripts/uncaught-run.js), com o ambiente
// {PATH, HOME, NO_COLOR, FOO, EMPTY, mixedCase} e argv extra `x --flag`. O resultado é
// `stdout + "#stderr\n" + stderr + "#exit " + código`, com o diretório trocado por /app e o pid por <pid>.
// Valores do host real (cwd, execPath, pid, ppid, tempos, memória, versão do bun) entram por forma, comparação ou
// regex; o sandbox finge Debian 13, então platform ('linux') e arch (a do bun) entram literais.
// Duas rodadas idênticas: linha que difere entre elas é descartada (vai para o stderr).
// Uso: bun scripts/gen-process-golden.js > tests/golden/process_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitFactored, assertPublicResult } = require("./golden-prelude.js");

const PRELUDE = "var L = function (x) { console.log(String(x)); };\nvar J = JSON.stringify;\n";
const REPEAT = 2;
const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};

function runChild(source) {
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "process-")));
  try {
    const mjs = source.includes("/*mjs*/");
    const file = path.join(dir, mjs ? "main.mjs" : "main.js");
    fs.writeFileSync(file, PRELUDE + source);
    const env = { PATH: process.env.PATH, HOME: dir, NO_COLOR: "1", FOO: "bar", EMPTY: "", mixedCase: "1" };
    const r = spawnSync(process.execPath, [file, "x", "--flag"], { cwd: dir, env, argv0: "bun", stdio: ["ignore", "pipe", "pipe"], timeout: 20000 });
    const norm = (buffer) =>
      buffer.toString("utf8").split(dir).join("/app").replace(/\((bun|node):\d+\)/g, "($1:<pid>)");
    return norm(r.stdout) + "#stderr\n" + norm(r.stderr) + "#exit " + (r.status === null ? "signal:" + r.signal : r.status);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------- nomes reais do bun, para os casos por chave
function probe(code) {
  const r = spawnSync(process.execPath, ["-e", code], { encoding: "utf8" });
  return JSON.parse(r.stdout);
}
const OWN = probe("console.log(JSON.stringify(Object.getOwnPropertyNames(process)))");
const PROTO = probe("console.log(JSON.stringify(Object.getOwnPropertyNames(Object.getPrototypeOf(process))))");
const DESC =
  "var D = function (o, k) { var d = Object.getOwnPropertyDescriptor(o, k); if (!d) return 'none'; " +
  "return ('value' in d ? 'v:' + typeof d.value : 'a:' + (d.get ? 'g' : '') + (d.set ? 's' : '')) + (d.writable ? ' w' : '') + (d.enumerable ? ' e' : '') + (d.configurable ? ' c' : ''); }; ";

// ---------------------------------------------------------------- 1. forma de process
add(
  "L(typeof process); L(Object.prototype.toString.call(process)); L(process[Symbol.toStringTag]);",
  "L(J(Object.getOwnPropertyNames(process)));",
  "L(J(Object.keys(process)));",
  "L(J(Object.getOwnPropertySymbols(process).map(String)));",
  "var P = Object.getPrototypeOf(process); L(J(Object.getOwnPropertyNames(P))); L(P.constructor.name); L(typeof P.constructor);",
  "var P = Object.getPrototypeOf(process); var Q = Object.getPrototypeOf(P); L(Q === require('events').prototype); L(Q === require('events').EventEmitter.prototype);",
  "var E = require('events'); L(process instanceof E); L(process instanceof E.EventEmitter);",
  "L(J(Object.getOwnPropertyDescriptor(globalThis, 'process'), function (k, v) { return typeof v === 'object' ? 'obj' : v; }));",
  "L(Object.keys(globalThis).includes('process')); L(Object.getOwnPropertyNames(globalThis).includes('process'));",
  "L(process === globalThis.process); L(process === require('process')); L(process === require('node:process'));",
  "L(process.constructor.name); L(process.constructor === Object); L(process.hasOwnProperty('constructor'));",
  "L(J(Object.getOwnPropertyDescriptor(Object.getPrototypeOf(process), Symbol.toStringTag)));",
  "L(J(Object.getOwnPropertyDescriptor(process, Symbol.toStringTag)));",
  "L(Object.isExtensible(process)); L(Object.isFrozen(process)); L(Object.isSealed(process));",
  "var p = process; process = 5; L(typeof process); process = p; L(typeof process);",
  "L(process.toString()); L(String(process)); L(process + '');",
  "L(J(process.eventNames())); L(process.getMaxListeners());",
  "L(J(Object.keys(process).slice(0, 5)));",
  "L(typeof process.on); L(typeof process.emit); L(typeof process.once); L(typeof process.off); L(typeof process.addListener);",
  "L(process.on('x', function () {}) === process); L(process.once('x', function () {}) === process); L(process.off('x', function () {}) === process);",
  "var n = 0; process.on('foo', function (a, b) { n += a + b; }); L(process.emit('foo', 1, 2)); L(n); L(process.emit('bar')); L(process.listenerCount('foo'));",
  "process.on('foo', function () { L(this === process); });  process.emit('foo');",
  "process.once('foo', function () { L('once'); }); L(process.emit('foo')); L(process.emit('foo'));",
  "L(typeof process.setMaxListeners); process.setMaxListeners(3); L(process.getMaxListeners());",
  "L(process.listenerCount('exit')); L(process.listenerCount('uncaughtException')); L(process.listenerCount('unhandledRejection')); L(process.listenerCount('SIGINT'));",
  "L(J(process.eventNames()));"
);
for (const k of OWN) {
  if (/[^\w$]/.test(k)) continue;
  add(DESC + `L(D(process, ${JSON.stringify(k)})); L(typeof process[${JSON.stringify(k)}]);`);
}
for (const k of PROTO) {
  add(DESC + `L(D(Object.getPrototypeOf(process), ${JSON.stringify(k)}));`);
}
for (const f of ["nextTick", "exit", "cwd", "chdir", "hrtime", "memoryUsage", "emitWarning", "uptime", "cpuUsage", "umask", "kill", "abort", "reallyExit", "binding", "getuid", "resourceUsage"]) {
  add(`var f = process.${f}; L(typeof f); if (typeof f === 'function') { L(f.name); L(f.length); L(f.hasOwnProperty('prototype')); L(Function.prototype.toString.call(f).slice(0, 40)); }`);
}
add(
  "L(typeof process.hrtime.bigint); L(process.hrtime.bigint.name); L(process.hrtime.bigint.length); L(typeof process.memoryUsage.rss); L(process.memoryUsage.rss.name);",
  "try { new process.nextTick(function () {}); L('no-throw'); } catch (e) { L(e.name + ': ' + e.message); }"
);

// ---------------------------------------------------------------- 2. nextTick: ordem
const ORDER = {
  "tick vs then": "Promise.resolve().then(function () { L('then'); }); process.nextTick(function () { L('tick'); }); L('sync');",
  "tick first": "process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); }); L('sync');",
  "tick vs queueMicrotask": "queueMicrotask(function () { L('qm'); }); process.nextTick(function () { L('tick'); }); L('sync');",
  "tick vs timeout": "setTimeout(function () { L('timeout'); }, 0); process.nextTick(function () { L('tick'); }); L('sync');",
  "tick vs immediate": "setImmediate(function () { L('imm'); }); process.nextTick(function () { L('tick'); }); L('sync');",
  "all four": "setTimeout(function () { L('timeout'); }, 0); setImmediate(function () { L('imm'); }); Promise.resolve().then(function () { L('then'); }); queueMicrotask(function () { L('qm'); }); process.nextTick(function () { L('tick'); }); L('sync');",
  "timeout vs immediate main": "setTimeout(function () { L('timeout'); }, 0); setImmediate(function () { L('imm'); });",
  "timeout vs immediate in io": "setTimeout(function () { setTimeout(function () { L('timeout'); }, 0); setImmediate(function () { L('imm'); }); }, 1);",
  "nested ticks": "process.nextTick(function () { L('a'); process.nextTick(function () { L('c'); }); }); process.nextTick(function () { L('b'); });",
  "tick in then": "Promise.resolve().then(function () { L('then1'); process.nextTick(function () { L('tick'); }); }).then(function () { L('then2'); });",
  "tick in then, second then queued": "Promise.resolve().then(function () { L('t1'); process.nextTick(function () { L('tick'); }); }); Promise.resolve().then(function () { L('t2'); }); Promise.resolve().then(function () { L('t3'); });",
  "then in tick": "process.nextTick(function () { L('tick1'); Promise.resolve().then(function () { L('then'); }); }); process.nextTick(function () { L('tick2'); });",
  "microtask in tick vs next tick": "process.nextTick(function () { L('tick1'); queueMicrotask(function () { L('qm'); }); process.nextTick(function () { L('tick3'); }); }); process.nextTick(function () { L('tick2'); });",
  "tick in microtask in tick": "process.nextTick(function () { Promise.resolve().then(function () { L('m'); process.nextTick(function () { L('t2'); }); Promise.resolve().then(function () { L('m2'); }); }); });",
  "async await vs tick": "(async function () { L('a1'); await null; L('a2'); await null; L('a3'); })(); process.nextTick(function () { L('tick'); }); L('sync');",
  "async await tick inside": "(async function () { await null; process.nextTick(function () { L('tick'); }); L('a2'); await null; L('a3'); })();",
  "tick before async continuation": "process.nextTick(function () { L('tick'); }); (async function () { await 1; L('after-await'); })(); L('sync');",
  "resolved then chain and tick": "var p = Promise.resolve(); p.then(function () { L('1'); }).then(function () { L('2'); }).then(function () { L('3'); }); process.nextTick(function () { L('tick'); });",
  "tick in timeout": "setTimeout(function () { L('t1'); process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); }); }, 0); setTimeout(function () { L('t2'); }, 0);",
  "tick in immediate": "setImmediate(function () { L('i1'); process.nextTick(function () { L('tick'); }); }); setImmediate(function () { L('i2'); });",
  "then in immediate": "setImmediate(function () { L('i1'); Promise.resolve().then(function () { L('then'); }); }); setImmediate(function () { L('i2'); });",
  "then in timeout": "setTimeout(function () { L('t1'); Promise.resolve().then(function () { L('then'); }); }, 0); setTimeout(function () { L('t2'); }, 0);",
  "tick between timers same expiry": "setTimeout(function () { L('a'); process.nextTick(L, 'ta'); }, 1); setTimeout(function () { L('b'); process.nextTick(L, 'tb'); }, 1); setTimeout(function () { L('c'); }, 1);",
  "tick between immediates": "setImmediate(function () { L('a'); process.nextTick(L, 'ta'); }); setImmediate(function () { L('b'); process.nextTick(L, 'tb'); }); setImmediate(function () { L('c'); });",
  "immediate scheduled in immediate": "setImmediate(function () { L('a'); setImmediate(function () { L('c'); }); }); setImmediate(function () { L('b'); });",
  "tick in immediate scheduled timeout": "setImmediate(function () { setTimeout(function () { L('timeout'); }, 0); process.nextTick(function () { L('tick'); }); });",
  "tick in tick in timeout": "setTimeout(function () { process.nextTick(function () { L('a'); process.nextTick(function () { L('b'); }); }); Promise.resolve().then(function () { L('m'); }); }, 0);",
  "many ticks fifo": "for (var i = 0; i < 5; i++) process.nextTick(L, i);",
  "tick, promise, tick fifo": "process.nextTick(L, 't1'); Promise.resolve().then(function () { L('p1'); }); process.nextTick(L, 't2'); Promise.resolve().then(function () { L('p2'); });",
  "promise rejection and tick": "Promise.reject(new Error('x')).catch(function () { L('caught'); }); process.nextTick(function () { L('tick'); });",
  "thenable resolution and tick": "var th = { then: function (r) { L('then called'); r(1); } }; Promise.resolve(th).then(function () { L('resolved'); }); process.nextTick(function () { L('tick'); });",
  "await thenable and tick": "(async function () { await { then: function (r) { L('then called'); r(1); } }; L('after'); })(); process.nextTick(function () { L('tick'); });",
  "promise all and tick": "Promise.all([1, 2]).then(function () { L('all'); }); process.nextTick(function () { L('tick'); });",
  "tick queued in sync after await": "(async function () { process.nextTick(function () { L('tick'); }); await null; L('after'); })(); L('sync');",
  "tick vs Promise.try": "Promise.try(function () { L('try'); }).then(function () { L('then'); }); process.nextTick(function () { L('tick'); });",
  "tick vs setImmediate inside promise": "Promise.resolve().then(function () { setImmediate(function () { L('imm'); }); process.nextTick(function () { L('tick'); }); L('then'); });",
  "ESM top level": "/*mjs*/ process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); }); queueMicrotask(function () { L('qm'); }); L('sync');",
  "ESM top level await": "/*mjs*/ process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); }); await null; L('after-await'); process.nextTick(function () { L('tick2'); });",
  "ESM top level await two": "/*mjs*/ process.nextTick(function () { L('tick'); }); await new Promise(function (r) { setTimeout(r, 1); }); L('after-timer'); process.nextTick(function () { L('tick2'); }); Promise.resolve().then(function () { L('then'); });",
  "ESM tick vs timeout": "/*mjs*/ setTimeout(function () { L('timeout'); }, 0); setImmediate(function () { L('imm'); }); process.nextTick(function () { L('tick'); });",
  "require vm eval tick": "globalThis.L = L; require('vm').runInThisContext('process.nextTick(function () { L(\"tick\"); }); Promise.resolve().then(function () { L(\"then\"); });'); L('after-vm');",
  "eval tick": "globalThis.L = L; (0, eval)('process.nextTick(function () { L(\"tick\"); });'); L('after');"
};
for (const body of Object.values(ORDER)) add(body);

// ---------------------------------------------------------------- 3. nextTick: argumentos, this, erros, recursão
add(
  "process.nextTick(function (a, b, c) { L(J([a, b, c])); L(arguments.length); }, 1, 2, 3);",
  "process.nextTick(function () { L(arguments.length); });",
  "process.nextTick(function () { L(arguments.length); L(J(Array.prototype.slice.call(arguments))); }, 1, 2, 3, 4, 5, 6);",
  "process.nextTick(function () { L(this === globalThis); L(this === undefined); L(typeof this); });",
  "process.nextTick(function () { 'use strict'; L(this === undefined); });",
  "process.nextTick(function () { L(this === process); });",
  "process.nextTick(() => { L(typeof this); });",
  "var o = { m: function () { L(this === o); } }; process.nextTick(o.m);",
  "var o = { m: function () { L(this === o); } }; process.nextTick(o.m.bind(o));",
  "L(String(process.nextTick(function () {})));",
  "L(typeof process.nextTick(function () {}));",
  "process.nextTick(L, 'a', 'b');",
  "process.nextTick(console.log, 'x', 'y');",
  "var f = function () { L('ran'); }; process.nextTick(f); process.nextTick(f);",
  "process.nextTick(async function () { L('async1'); await null; L('async2'); }); L('sync');",
  "process.nextTick(function* () { L('never'); }); L('gen-ok');",
  "class A { static m() { L('static'); } } process.nextTick(A.m);",
  "try { process.nextTick(class A {}); L('no-throw'); } catch (e) { L(e.name + ': ' + e.message); }",
  "process.nextTick(new Proxy(function () { L('proxy'); }, {}));",
  "process.nextTick(function () { L(new Error().stack.split('\\n')[0]); });",
  "process.nextTick(function () { L(typeof new Error().stack); });",
  "var e = new Error('x'); process.nextTick(function () { L(e.stack.split('\\n').length > 1); });",
  "L(process.nextTick.length); L(process.nextTick.name);"
);
for (const bad of ["undefined", "null", "'str'", "123", "{}", "[]", "Symbol('s')", "true", "1n", "{ call: function () {} }"]) {
  add(`try { process.nextTick(${bad}); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); L(e.constructor === TypeError); L(J(Object.getOwnPropertyNames(e))); L(e.stack.split('\\n')[0]); }`);
}
add(
  "try { process.nextTick(); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.nextTick(undefined, 1, 2); L('no-throw'); } catch (e) { L(e.code); }",
  "try { process.nextTick(function () {}); L('ok'); } catch (e) { L(e.code); }",
  "var e; try { process.nextTick('x'); } catch (x) { e = x; } L(e instanceof TypeError); L(Object.prototype.toString.call(e)); L(J(Object.getOwnPropertyDescriptor(e, 'code'))); L(e.name); L(String(e));"
);
// erro dentro do tick
add(
  "process.nextTick(function () { throw new Error('boom'); }); setTimeout(function () { L('timeout'); }, 5);",
  "process.nextTick(function () { throw new Error('boom'); }); process.nextTick(function () { L('second'); });",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); process.nextTick(function () { throw new Error('boom'); }); process.nextTick(function () { L('second'); }); setTimeout(function () { L('timeout'); }, 5);",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); process.nextTick(function () { throw new Error('boom'); }); Promise.resolve().then(function () { L('then'); }); setImmediate(function () { L('imm'); });",
  "process.on('uncaughtException', function (e) { L('handler ' + e); }); process.nextTick(function () { throw 'str'; }); process.nextTick(function () { throw 42; }); process.nextTick(function () { throw null; });",
  "process.on('uncaughtException', function (e) { L('handler ' + e.message); }); process.nextTick(function () { process.nextTick(function () { L('inner'); }); throw new Error('boom'); }); process.nextTick(function () { L('outer-second'); });",
  "process.on('uncaughtException', function (e) { L('handler ' + e.message); }); process.nextTick(function () { Promise.resolve().then(function () { L('m'); }); throw new Error('boom'); });",
  "process.on('uncaughtException', function (e) { L('handler ' + e.message); throw new Error('from handler'); }); process.nextTick(function () { throw new Error('boom'); }); setTimeout(function () { L('timeout'); }, 5);",
  "process.nextTick(function () { try { throw new Error('inner'); } catch (e) { L('caught ' + e.message); } });",
  "process.nextTick(function () { Promise.reject(new Error('rej')); }); setTimeout(function () { L('timeout'); }, 5);",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); process.nextTick(function () { Promise.reject(new Error('rej')); });",
  "process.nextTick(async function () { throw new Error('async boom'); });",
  "process.on('unhandledRejection', function (r, p) { L('unhandled ' + r.message + ' ' + (p instanceof Promise)); }); process.nextTick(async function () { throw new Error('async boom'); });"
);
// recursão e inanição
add(
  "var n = 0; function f() { if (++n < 10000) process.nextTick(f); else L('done ' + n); } process.nextTick(f);",
  "var n = 0; function f() { if (++n < 100000) process.nextTick(f); else L('done ' + n); } process.nextTick(f);",
  "var n = 0; setTimeout(function () { L('timeout at ' + n); }, 0); function f() { if (++n < 1000) process.nextTick(f); else L('done ' + n); } process.nextTick(f);",
  "var n = 0; setImmediate(function () { L('imm at ' + n); }); function f() { if (++n < 1000) process.nextTick(f); else L('done ' + n); } process.nextTick(f);",
  "var n = 0; Promise.resolve().then(function () { L('then at ' + n); }); function f() { if (++n < 1000) process.nextTick(f); else L('done ' + n); } process.nextTick(f);",
  "var n = 0; function f() { if (++n < 1000) Promise.resolve().then(function () { process.nextTick(f); }); else L('done ' + n); } process.nextTick(f); setTimeout(function () { L('timeout ' + n); }, 0);",
  "var n = 0; function f() { n++; if (n < 5) { process.nextTick(f); Promise.resolve().then(function () { L('m' + n); }); } } process.nextTick(f);",
  "var n = 0; function f() { n++; if (n < 4) { process.nextTick(f); process.nextTick(f); } } process.nextTick(f); setTimeout(function () { L('count ' + n); }, 5);",
  "function f(d) { if (d < 3) { process.nextTick(f, d + 1); L('d' + d); } } process.nextTick(f, 0);",
  "var depth = 0; (function f() { process.nextTick(function () { depth++; if (depth < 3) f(); }); })(); setTimeout(function () { L('depth ' + depth); }, 5);",
  "var t = []; for (var i = 0; i < 1000; i++) process.nextTick(function (i) { t.push(i); }, i); process.nextTick(function () { L(t.length); L(t[0]); L(t[999]); });"
);

// ---------------------------------------------------------------- 4. exitCode, exit, eventos de saída
add(
  "L(String(process.exitCode)); L(typeof process.exitCode);",
  "process.exitCode = 5;",
  "process.exitCode = 5; process.on('exit', function (c) { L('exit ' + c + ' ' + process.exitCode); });",
  "process.exitCode = 5; process.exit();",
  "process.exitCode = 5; process.exit(7);",
  "process.exit(7); L('never');",
  "process.exit(); L('never');",
  "process.exit(0);",
  "process.exit(256);",
  "process.exit(257);",
  "process.exit(-1);",
  "process.exit(255);",
  "process.exit(1.5);",
  "process.exit('3');",
  "process.exit('abc');",
  "process.exit(true);",
  "process.exit(null);",
  "process.exit(undefined);",
  "process.exit({});",
  "process.exit(NaN);",
  "process.exit(Infinity);",
  "process.exit(2 ** 32 + 3);",
  "process.exit(1n);",
  "try { process.exit('abc'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.exitCode = 'abc'; L('ok'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.exitCode = 1.5; L('ok'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.exitCode = {}; L('ok'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.exitCode = -1; L('ok ' + process.exitCode); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "process.exitCode = '4'; L(typeof process.exitCode); L(process.exitCode);",
  "process.exitCode = 300; L(process.exitCode);",
  "process.exitCode = null; L(String(process.exitCode));",
  "process.exitCode = undefined; L(String(process.exitCode));",
  "process.exitCode = 3; process.exitCode = undefined; L(String(process.exitCode));",
  "process.exitCode = 3; delete process.exitCode; L(String(process.exitCode));",
  "L(J(Object.getOwnPropertyDescriptor(process, 'exitCode')));",
  "process.exitCode = 3; L(J(Object.getOwnPropertyDescriptor(process, 'exitCode')));",
  "process.exitCode = 9; process.on('exit', function (c) { process.exitCode = 4; L('exit ' + c); });",
  "process.on('exit', function (c) { L('exit ' + c); process.exitCode = 6; });",
  "process.on('exit', function (c) { L('exit ' + c); process.exit(8); });",
  "process.on('exit', function (c) { L('exit ' + c); process.exit(); });",
  "process.on('exit', function (c) { L('first ' + c); }); process.on('exit', function (c) { L('second ' + c); }); process.exit(2);",
  "process.on('exit', function (c) { L('exit ' + c); throw new Error('in exit'); });",
  "process.on('exit', function (c) { L('exit ' + c); }); throw new Error('uncaught');",
  "process.on('exit', function (c) { L('exit ' + c + ' ' + process.exitCode); }); Promise.reject(new Error('rej'));",
  "process.on('exit', function () { L('exit'); setTimeout(function () { L('timer in exit'); }, 0); process.nextTick(function () { L('tick in exit'); }); Promise.resolve().then(function () { L('then in exit'); }); setImmediate(function () { L('imm in exit'); }); });",
  "process.on('exit', function () { L('exit sync only'); }); process.nextTick(function () { L('tick'); process.exit(); }); process.nextTick(function () { L('second tick'); });",
  "process.nextTick(function () { L('tick'); }); process.exit(); ",
  "Promise.resolve().then(function () { L('then'); }); process.exit();",
  "setTimeout(function () { L('timeout'); }, 0); process.exit();",
  "setImmediate(function () { L('imm'); }); process.exit();",
  "process.on('exit', function () { L('exit1'); }); process.once('exit', function () { L('exit-once'); }); process.exit();",
  "process.on('exit', function () { L('exit'); }); process.emit('exit', 9); L('after emit'); process.emit('exit', 9);",
  "process.on('exit', function (c) { L('exit ' + c + ' ' + arguments.length); });",
  "var n = 0; process.on('exit', function () { n++; L('exit ' + n); process.exit(); });",
  "process.on('exit', function () { L('exit'); }); setTimeout(function () { process.exit(3); }, 5); setTimeout(function () { L('never'); }, 50);",
  "process.on('exit', function () { L('exit'); }); setInterval(function () {}, 1000).unref(); L('end');",
  "process.on('exit', function () { L('exit'); }); var t = setInterval(function () { L('tick'); clearInterval(t); }, 1);",
  "process.reallyExit && L(typeof process.reallyExit); process.reallyExit(4);",
  "L(typeof process.exit); process.exit(process.exitCode);",
  "process.exitCode = 2; process.on('exit', function (c) { L(c); process.exit(); });",
  "process.exitCode = 2; process.on('exit', function (c) { L(c); }); process.exit(0);",
  "process.on('exit', function (c) { L('code ' + c); }); process.exit(300);",
  "process.on('exit', function (c) { L('code ' + c + ' ' + typeof c); });",
  "process.on('exit', function (c) { L('code ' + c); }); process.exitCode = '7';"
);
// beforeExit
add(
  "process.on('beforeExit', function (c) { L('beforeExit ' + c); });",
  "process.on('beforeExit', function (c) { L('beforeExit ' + c); }); process.on('exit', function (c) { L('exit ' + c); });",
  "var n = 0; process.on('beforeExit', function (c) { L('beforeExit ' + c + ' ' + n); if (n++ < 2) setTimeout(function () { L('timer'); }, 1); });",
  "var n = 0; process.on('beforeExit', function () { L('beforeExit ' + n); if (n++ < 2) process.nextTick(function () { L('tick'); }); });",
  "var n = 0; process.on('beforeExit', function () { L('beforeExit ' + n); if (n++ < 2) Promise.resolve().then(function () { L('then'); }); });",
  "var n = 0; process.on('beforeExit', function () { L('beforeExit ' + n); if (n++ < 2) setImmediate(function () { L('imm'); }); });",
  "var n = 0; process.on('beforeExit', function () { L('beforeExit ' + n); if (n++ < 1) queueMicrotask(function () { L('qm'); }); });",
  "process.on('beforeExit', function () { L('beforeExit'); }); process.exit();",
  "process.on('beforeExit', function () { L('beforeExit'); }); throw new Error('x');",
  "process.on('beforeExit', function () { L('beforeExit'); }); Promise.reject(new Error('x'));",
  "process.on('beforeExit', function (c) { L('beforeExit ' + c + ' ' + process.exitCode); }); process.exitCode = 3;",
  "process.on('beforeExit', function (c) { L('beforeExit ' + c); process.exitCode = 4; }); process.on('exit', function (c) { L('exit ' + c); });",
  "process.on('beforeExit', function () { L('beforeExit'); process.exit(5); }); process.on('exit', function (c) { L('exit ' + c); });",
  "process.on('beforeExit', function () { L('b1'); }); process.on('beforeExit', function () { L('b2'); });",
  "process.once('beforeExit', function () { L('once'); setTimeout(function () { L('timer'); }, 1); });",
  "process.on('beforeExit', function () { L('beforeExit'); }); setTimeout(function () { L('timer'); }, 5);",
  "process.on('beforeExit', function () { L('beforeExit'); }); setInterval(function () {}, 1000).unref();",
  "process.on('beforeExit', function () { L('beforeExit'); throw new Error('in beforeExit'); });",
  "process.on('beforeExit', function () { L('beforeExit'); }); process.emit('beforeExit', 0); L('after');",
  "process.on('beforeExit', function () { L('beforeExit'); }); (async function () { await new Promise(function (r) { setTimeout(r, 1); }); L('awaited'); })();",
  "/*mjs*/ process.on('beforeExit', function (c) { L('beforeExit ' + c); }); await null; L('tla');"
);

// ---------------------------------------------------------------- 5. uncaughtException
add(
  "throw new Error('boom');",
  "throw 'str';",
  "throw 42;",
  "throw null;",
  "throw undefined;",
  "throw { a: 1 };",
  "throw new TypeError('bad type');",
  "var e = new Error('with props'); e.code = 'E_X'; e.extra = 1; throw e;",
  "setTimeout(function () { throw new Error('in timer'); }, 0);",
  "setImmediate(function () { throw new Error('in immediate'); });",
  "Promise.resolve().then(function () { throw new Error('in then'); });",
  "queueMicrotask(function () { throw new Error('in qm'); });",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin + ' ' + arguments.length); }); throw new Error('boom');",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); throw new Error('boom'); L('never');",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); setTimeout(function () { throw new Error('t'); }, 0); setTimeout(function () { L('second timer'); }, 0);",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); setImmediate(function () { throw new Error('i'); }); setImmediate(function () { L('second imm'); });",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); queueMicrotask(function () { throw new Error('qm'); }); queueMicrotask(function () { L('second qm'); });",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); Promise.resolve().then(function () { throw new Error('then'); }); Promise.resolve().then(function () { L('second then'); });",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); Promise.reject(new Error('rej'));",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + String(e) + ' ' + origin); }); Promise.reject('str');",
  "process.on('uncaughtException', function (e, origin) { L('handler ' + e.message + ' ' + origin); }); L('sync end');",
  "process.on('uncaughtException', function (e) { L('h1 ' + e.message); }); process.on('uncaughtException', function (e) { L('h2 ' + e.message); }); throw new Error('boom');",
  "process.once('uncaughtException', function (e) { L('once ' + e.message); }); throw new Error('first'); ",
  "process.once('uncaughtException', function (e) { L('once ' + e.message); }); setTimeout(function () { throw new Error('first'); }, 0); setTimeout(function () { throw new Error('second'); }, 1);",
  "var h = function (e) { L('h ' + e.message); }; process.on('uncaughtException', h); process.off('uncaughtException', h); throw new Error('boom');",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); throw new Error('second'); }); throw new Error('first');",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); process.exit(9); }); throw new Error('first');",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); process.exitCode = 9; }); throw new Error('first');",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); }); process.on('exit', function (c) { L('exit ' + c); }); throw new Error('first');",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); }); process.on('exit', function (c) { L('exit ' + c); }); setTimeout(function () { throw new Error('first'); }, 0);",
  "process.on('uncaughtExceptionMonitor', function (e, origin) { L('monitor ' + e.message + ' ' + origin); }); throw new Error('boom');",
  "process.on('uncaughtExceptionMonitor', function (e, origin) { L('monitor ' + e.message + ' ' + origin); }); process.on('uncaughtException', function (e) { L('handler ' + e.message); }); throw new Error('boom');",
  "process.on('uncaughtExceptionMonitor', function (e, origin) { L('monitor ' + origin); }); Promise.reject(new Error('rej'));",
  "L(typeof process.setUncaughtExceptionCaptureCallback); L(process.hasUncaughtExceptionCaptureCallback());",
  "process.setUncaughtExceptionCaptureCallback(function (e) { L('capture ' + e.message); }); L(process.hasUncaughtExceptionCaptureCallback()); throw new Error('boom');",
  "process.setUncaughtExceptionCaptureCallback(function (e) { L('capture ' + e.message); }); process.on('uncaughtException', function () { L('handler'); }); throw new Error('boom');",
  "process.setUncaughtExceptionCaptureCallback(function () {}); try { process.setUncaughtExceptionCaptureCallback(function () {}); } catch (e) { L(e.code); L(e.message); }",
  "try { process.setUncaughtExceptionCaptureCallback(5); } catch (e) { L(e.code); L(e.message); }",
  "process.setUncaughtExceptionCaptureCallback(function () {}); process.setUncaughtExceptionCaptureCallback(null); L(process.hasUncaughtExceptionCaptureCallback());",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); }); process.nextTick(function () { throw new Error('tick'); }); L('sync');",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); }); setTimeout(function () { L('a'); }, 0); setTimeout(function () { throw new Error('t'); }, 0); setTimeout(function () { L('c'); }, 0);",
  "process.on('uncaughtException', function (e) { L('h ' + e.stack.split('\\n')[0]); }); null.x;",
  "process.on('uncaughtException', function (e) { L('h ' + e.name + ': ' + e.message); }); undefinedFn();",
  "process.on('uncaughtException', function (e) { L('h ' + e.name + ': ' + e.message); }); (function f() { f(); })();",
  "process.on('uncaughtException', function (e) { L('h ' + (e instanceof Error)); }); throw Object.create(null);",
  "process.on('uncaughtException', function () { L('h'); }); var n = 0; setInterval(function () { if (++n === 4) process.exit(); throw new Error('i'); }, 1);",
  "process.on('uncaughtException', function (e) { L('h ' + e.message); }); async function f() { throw new Error('async'); } f();",
  "process.on('uncaughtException', function (e, o) { L('h ' + e.message + ' ' + o); }); new Promise(function () { throw new Error('executor'); });",
  "process.on('uncaughtException', function (e, o) { L('h ' + e.message + ' ' + o); }); process.emit('uncaughtException', new Error('manual'), 'x');",
  "process.emit('uncaughtException', new Error('manual no listener')); L('after');",
  "var t = new EventTarget(); t.addEventListener('x', function () { throw new Error('l1'); }); L(t.dispatchEvent(new Event('x'))); L('after');",
  "var t = new EventTarget(); t.addEventListener('x', function () { throw new Error('l1'); }); t.addEventListener('x', function () { throw new Error('l2'); }); L(t.dispatchEvent(new Event('x'))); L('after');",
  "process.on('uncaughtException', function (e, o) { L('h ' + e.message + ' ' + o); }); var t = new EventTarget(); t.addEventListener('x', function () { throw new Error('l1'); }); L(t.dispatchEvent(new Event('x'))); L('after');",
  "process.on('uncaughtException', function (e, o) { L('h ' + e.message + ' ' + o); }); var t = new EventTarget(); t.addEventListener('x', function () { throw new Error('l1'); }); t.addEventListener('x', function () { throw new Error('l2'); }); L(t.dispatchEvent(new Event('x'))); L('after');"
);

// ---------------------------------------------------------------- 6. unhandledRejection, rejectionHandled, warning
add(
  "Promise.reject(new Error('rej'));",
  "Promise.reject('str');",
  "Promise.reject(42);",
  "Promise.reject(undefined);",
  "Promise.reject({ a: 1 });",
  "Promise.reject(null);",
  "Promise.reject(new Error('rej')); setTimeout(function () { L('timer'); }, 5);",
  "(async function () { throw new Error('async'); })();",
  "new Promise(function (_, rej) { rej(new Error('exec')); });",
  "var p = Promise.reject(new Error('rej')); p.catch(function () {}); L('handled');",
  "var p = Promise.reject(new Error('rej')); setTimeout(function () { p.catch(function () { L('late catch'); }); }, 1); setTimeout(function () { L('timer'); }, 20);",
  "process.on('unhandledRejection', function (r, p) { L('unhandled ' + r.message + ' ' + (p instanceof Promise) + ' ' + arguments.length); }); Promise.reject(new Error('rej'));",
  "process.on('unhandledRejection', function (r, p) { L('unhandled ' + String(r)); }); Promise.reject('str'); Promise.reject(1); Promise.reject(undefined);",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.reject(new Error('a')); Promise.reject(new Error('b')); L('sync');",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.reject(new Error('a')); process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); }); setImmediate(function () { L('imm'); }); setTimeout(function () { L('timeout'); }, 0);",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.reject(new Error('a')).catch(function () { L('caught'); });",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.reject(new Error('a')).then(function () {}); ",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.reject(new Error('a')).finally(function () {});",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.resolve().then(function () { throw new Error('in then'); });",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); (async function () { await null; throw new Error('after await'); })();",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.all([Promise.reject(new Error('all'))]);",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.race([Promise.reject(new Error('race'))]).catch(function () {});",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.allSettled([Promise.reject(new Error('settled'))]).then(function () { L('settled done'); });",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); throw new Error('from handler'); }); Promise.reject(new Error('a'));",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); throw new Error('from handler'); }); process.on('uncaughtException', function (e, o) { L('uncaught ' + e.message + ' ' + o); }); Promise.reject(new Error('a'));",
  "process.on('uncaughtException', function (e, o) { L('uncaught ' + e.message + ' ' + o); }); Promise.reject(new Error('a'));",
  "process.on('uncaughtException', function (e, o) { L('uncaught ' + String(e) + ' ' + o + ' ' + (e instanceof Error) + ' ' + e.code); }); Promise.reject('str');",
  "process.on('uncaughtException', function (e, o) { L('uncaught ' + e.name + ' ' + e.code + ' ' + o); }); Promise.reject(42);",
  "process.on('rejectionHandled', function (p) { L('rejectionHandled ' + (p instanceof Promise)); }); process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); var p = Promise.reject(new Error('a')); setTimeout(function () { p.catch(function () {}); }, 5);",
  "process.on('rejectionHandled', function () { L('rejectionHandled'); }); var p = Promise.reject(new Error('a')); p.catch(function () {}); ",
  "process.on('unhandledRejection', function (r, p) { L('unhandled'); p.catch(function () { L('handled inside'); }); }); Promise.reject(new Error('a'));",
  "process.on('rejectionHandled', function () { L('rejectionHandled'); }); process.on('unhandledRejection', function (r, p) { L('unhandled'); p.catch(function () { L('handled inside'); }); }); Promise.reject(new Error('a'));",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); var p = Promise.reject(new Error('a')); p.then(function () {}, function () { L('rejected cb'); });",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); (async function () { try { await Promise.reject(new Error('a')); } catch (e) { L('caught ' + e.message); } })();",
  "process.on('unhandledRejection', function (r) { L('unhandled ' + r.message); }); Promise.reject(new Error('a')); process.exitCode = 3;",
  "process.on('unhandledRejection', function (r) { L('unhandled'); }); process.on('exit', function (c) { L('exit ' + c); }); Promise.reject(new Error('a'));",
  "process.on('unhandledRejection', function (r) { L('unhandled'); }); setTimeout(function () { Promise.reject(new Error('t')); }, 0);",
  "process.on('unhandledRejection', function (r) { L('unhandled'); }); setImmediate(function () { Promise.reject(new Error('i')); });",
  "process.on('unhandledRejection', function (r) { L('unhandled'); }); process.nextTick(function () { Promise.reject(new Error('t')); });",
  "process.on('unhandledRejection', function (r) { L('unhandled'); }); Promise.reject(new Error('a')); Promise.reject(new Error('b')); process.on('unhandledRejection', function () { L('second'); });",
  "process.once('unhandledRejection', function (r) { L('once ' + r.message); }); Promise.reject(new Error('a')); Promise.reject(new Error('b'));",
  "process.on('unhandledRejection', function (r) { L('unhandled'); }); (async function () { await Promise.reject(new Error('a')); })().catch(function () { L('outer caught'); });"
);
// emitWarning e warning
add(
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code + '|' + (w instanceof Error)); }); process.emitWarning('hello');",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code); }); process.emitWarning('hello', 'CustomWarning');",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code); }); process.emitWarning('hello', 'CustomWarning', 'CODE1');",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code); }); process.emitWarning('hello', { type: 'TypeX', code: 'C2', detail: 'some detail' });",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code + '|' + w.detail); }); process.emitWarning('hello', { type: 'TypeX', code: 'C2', detail: 'some detail' });",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code); }); process.emitWarning('dep', 'DeprecationWarning', 'DEP0001');",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code); }); var e = new Error('given'); e.name = 'MyWarn'; process.emitWarning(e);",
  "process.on('warning', function (w) { L(w === globalThis.__e); }); globalThis.__e = new Error('given'); process.emitWarning(globalThis.__e);",
  "process.on('warning', function (w) { L(w.name + '|' + w.message + '|' + w.code); }); process.emitWarning(new TypeError('given type'));",
  "process.on('warning', function (w) { L(w.constructor.name + ' ' + J(Object.getOwnPropertyNames(w))); }); process.emitWarning('hello');",
  "process.on('warning', function (w) { L(w.stack.split('\\n')[0]); }); process.emitWarning('hello');",
  "process.on('warning', function (w) { L(w.stack.split('\\n').length > 1); }); process.emitWarning('hello');",
  "process.on('warning', function () { L('warning event'); }); process.emitWarning('hello'); L('sync after'); process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); });",
  "process.on('warning', function () { L('warning event'); }); process.emitWarning('hello'); setImmediate(function () { L('imm'); }); setTimeout(function () { L('timeout'); }, 0);",
  "process.on('warning', function () { L('w'); }); process.emitWarning('a'); process.emitWarning('b'); process.emitWarning('c');",
  "process.on('warning', function (w) { L(w.message); }); process.emitWarning('same'); process.emitWarning('same');",
  "process.on('warning', function (w) { L(w.message); }); process.emitWarning('dep1', 'DeprecationWarning', 'DEP_SAME'); process.emitWarning('dep2', 'DeprecationWarning', 'DEP_SAME');",
  "L(String(process.emitWarning('hello')));",
  "process.emitWarning('to stderr');",
  "process.emitWarning('to stderr', 'CustomWarning');",
  "process.emitWarning('to stderr', 'CustomWarning', 'CODE');",
  "process.emitWarning('to stderr', { type: 'T', code: 'C', detail: 'the detail' });",
  "process.emitWarning('to stderr', 'DeprecationWarning');",
  "process.emitWarning('to stderr', 'DeprecationWarning', 'DEP0999');",
  "process.emitWarning(new Error('as error'));",
  "var e = new Error('as error'); e.name = 'Custom'; process.emitWarning(e);",
  "process.emitWarning('exit code check'); process.on('exit', function (c) { L('exit ' + c); });",
  "process.noDeprecation = true; process.emitWarning('dep', 'DeprecationWarning'); process.emitWarning('other');",
  "process.on('warning', function (w) { L(w.message); }); process.noDeprecation = true; process.emitWarning('dep', 'DeprecationWarning'); process.emitWarning('other');",
  "L(String(process.noDeprecation)); L(String(process.throwDeprecation)); L(String(process.traceDeprecation));",
  "process.throwDeprecation = true; try { process.emitWarning('dep', 'DeprecationWarning'); L('no-throw'); } catch (e) { L('threw ' + e.name); } setTimeout(function () { L('timeout'); }, 5);",
  "process.on('uncaughtException', function (e) { L('uncaught ' + e.name + ' ' + e.message); }); process.throwDeprecation = true; process.emitWarning('dep', 'DeprecationWarning');",
  "try { process.emitWarning(); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning(5); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning(null); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning('x', 5); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning('x', 'T', 5); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning('x', { type: 5 }); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning('x', null); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.emitWarning('x', undefined, 'C'); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "process.on('warning', function (w) { L(w.name + '|' + w.code); }); process.emitWarning('x', function () {}); ",
  "process.on('warning', function (w) { L(w.name + '|' + w.message); }); process.emitWarning('x', 'T', function () {}); ",
  "process.on('warning', function (w) { L('h1 ' + w.name); }); process.on('warning', function (w) { L('h2 ' + w.name); }); process.emitWarning('x');",
  "process.on('warning', function (w) { throw new Error('in warning handler'); }); process.emitWarning('x'); setTimeout(function () { L('timeout'); }, 5);",
  "process.on('warning', function (w) { L(w.name); }); process.emit('warning', new Error('manual')); L('after');",
  "process.on('warning', function (w) { L(w.name + ' ' + w.message); }); new Buffer(1);",
  "process.on('warning', function (w) { L(w.name + ' ' + w.message); }); require('events').defaultMaxListeners = 1; var e = new (require('events'))(); e.on('x', function () {}); e.on('x', function () {});",
  "process.on('warning', function (w) { L(w.name + ' ' + w.message.replace(/0x[0-9a-f]+/g, '0x')); }); var e = new (require('events'))(); for (var i = 0; i < 11; i++) e.on('x', function () {});",
  "process.on('warning', function (w) { L(w.name + ' ' + w.message.replace(/0x[0-9a-f]+/g, '0x')); }); for (var i = 0; i < 11; i++) process.on('foo', function () {});",
  "process.on('warning', function (w) { L(w.name + ' ' + w.message); }); var e = new (require('events'))(); e.setMaxListeners(1); e.on('x', function () {}); e.on('x', function () {}); e.on('x', function () {});"
);

// ---------------------------------------------------------------- 7. env
add(
  "L(typeof process.env); L(Object.prototype.toString.call(process.env)); L(Object.getPrototypeOf(process.env) === Object.prototype); L(process.env.constructor === Object);",
  "L(J(Object.keys(process.env).sort()));",
  "L(process.env.FOO); L(J(process.env.EMPTY)); L(typeof process.env.EMPTY); L(String(process.env.MISSING)); L(process.env.mixedCase);",
  "L(String(process.env.foo)); L(String(process.env.Foo)); L(String(process.env.MIXEDCASE)); L(String(process.env.mixedcase)); L(String(process.env.fOO));",
  "L('FOO' in process.env); L('foo' in process.env); L('mixedcase' in process.env); L('MIXEDCASE' in process.env); L(process.env.hasOwnProperty('FOO')); L(process.env.hasOwnProperty('foo'));",
  "process.env.NEW = 'v'; L(process.env.NEW); L(process.env.new); L('NEW' in process.env); L('new' in process.env);",
  "process.env.NUM = 1; L(typeof process.env.NUM); L(process.env.NUM); process.env.B = true; L(typeof process.env.B); L(process.env.B);",
  "process.env.U = undefined; L(typeof process.env.U); L(process.env.U); L('U' in process.env);",
  "process.env.N = null; L(typeof process.env.N); L(process.env.N);",
  "process.env.O = {}; L(process.env.O); process.env.A = [1, 2]; L(process.env.A);",
  "process.env.F = function () {}; L(typeof process.env.F);",
  "process.env.S = 'a'; process.env.S = 'b'; L(process.env.S); delete process.env.S; L(String(process.env.S)); L('S' in process.env);",
  "L(delete process.env.NOPE); L(delete process.env.FOO); L(String(process.env.FOO)); L(delete process.env.foo);",
  "process.env.X = 'y'; L(J(Object.getOwnPropertyDescriptor(process.env, 'X')));",
  "L(J(Object.getOwnPropertyDescriptor(process.env, 'FOO')));",
  "L(J(Object.getOwnPropertyDescriptor(process.env, 'MISSING')));",
  "try { Object.defineProperty(process.env, 'D', { value: 'x' }); L('ok ' + process.env.D); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { Object.defineProperty(process.env, 'D', { value: 'x', configurable: true, writable: true, enumerable: true }); L('ok ' + process.env.D); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { Object.defineProperty(process.env, 'D', { get: function () { return 'x'; }, configurable: true, enumerable: true }); L('ok'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { Object.defineProperty(process.env, 'D', { value: 5, configurable: true, writable: true, enumerable: true }); L(typeof process.env.D); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.env[Symbol('s')] = 'x'; L('set ok'); } catch (e) { L(e.name); L(e.message); }",
  "try { L(String(process.env[Symbol('s')])); } catch (e) { L(e.name); L(e.message); }",
  "try { L(Symbol.iterator in process.env); } catch (e) { L(e.name); L(e.message); }",
  "L(J(Object.getOwnPropertySymbols(process.env)));",
  "L(J(Reflect.ownKeys(process.env).filter(function (k) { return typeof k === 'string'; }).sort()));",
  "var s = 0; for (var k in process.env) s++; L(s === Object.keys(process.env).length);",
  "L(Object.keys(process.env).length); L(Object.entries(process.env).length); L(Object.values(process.env).length);",
  "process.env.A1 = 'x'; var k = Object.keys(process.env); L(k[k.length - 1]); process.env.B1 = 'y'; k = Object.keys(process.env); L(k[k.length - 1]);",
  "L(J(Object.keys(process.env)));",
  "L(Object.isExtensible(process.env)); L(Object.isFrozen(process.env)); L(Object.isSealed(process.env));",
  "Object.freeze(process.env); L(Object.isFrozen(process.env)); process.env.Z = 'x'; L(String(process.env.Z));",
  "try { Object.freeze(process.env); L('frozen'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { Object.preventExtensions(process.env); L('ok ' + Object.isExtensible(process.env)); process.env.Q = 'x'; L(String(process.env.Q)); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { Object.seal(process.env); L('sealed'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { Object.setPrototypeOf(process.env, null); L('ok'); } catch (e) { L(e.name); L(e.message); }",
  "var e = process.env; e.A = 1; L(e === process.env); L(process.env === process.env);",
  "var c = Object.assign({}, process.env); c.FOO = 'changed'; L(process.env.FOO); L(c.FOO);",
  "var c = { ...process.env }; L(c.FOO); L(c.hasOwnProperty('FOO'));",
  "var c = structuredClone(process.env); L(c.FOO); L(Object.getPrototypeOf(c) === Object.prototype);",
  "L(J(process.env).includes('\"FOO\":\"bar\"')); L(J(process.env).includes('mixedCase'));",
  "L(process.env.toString()); L(String(process.env)); L(typeof process.env.valueOf); L(typeof process.env.hasOwnProperty); L(typeof process.env.toString);",
  "process.env.toString = 'x'; L(typeof process.env.toString); L(String(process.env.hasOwnProperty));",
  "process.env.hasOwnProperty = 'x'; L(typeof process.env.hasOwnProperty);",
  "process.env.__proto__ = 'x'; L(typeof process.env.__proto__); L(Object.getPrototypeOf(process.env) === Object.prototype);",
  "process.env.constructor = 'x'; L(typeof process.env.constructor);",
  "process.env['a b'] = 'c'; L(process.env['a b']); process.env['é'] = 'x'; L(process.env['é']);",
  "process.env['0'] = 'zero'; L(process.env[0]); L(typeof process.env[0]); L(Object.keys(process.env)[0]);",
  "process.env[''] = 'empty'; L(J(process.env[''])); L('' in process.env);",
  "process.env['A=B'] = 'x'; L(process.env['A=B']);",
  "process.env.NUL = 'a\\0b'; L(J(process.env.NUL));",
  "process.env.LONG = 'x'.repeat(100000); L(process.env.LONG.length);",
  "process.env.UNI = 'ação 日本'; L(process.env.UNI);",
  "process.env.NEWL = 'a\\nb'; L(J(process.env.NEWL));",
  "process.env.X = 1; var cp = require('child_process'); L(cp.execFileSync(process.execPath, ['-e', 'console.log(process.env.X + typeof process.env.X)'], { encoding: 'utf8' }).trim());",
  "process.env.X = 'child'; var cp = require('child_process'); L(cp.execFileSync(process.execPath, ['-e', 'console.log(process.env.X)'], { encoding: 'utf8' }).trim());",
  "process.env.FOO = 'changed'; var cp = require('child_process'); L(cp.execFileSync(process.execPath, ['-e', 'console.log(process.env.FOO)'], { encoding: 'utf8' }).trim());",
  "delete process.env.FOO; var cp = require('child_process'); L(cp.execFileSync(process.execPath, ['-e', 'console.log(String(process.env.FOO))'], { encoding: 'utf8' }).trim());",
  "L(J(Object.getOwnPropertyDescriptor(process, 'env'), function (k, v) { return typeof v === 'object' ? 'obj' : v; }));",
  "var e = process.env; process.env = { A: 1 }; L(process.env.A); L(process.env === e);",
  "L(process.env.NO_COLOR); L(typeof process.env.HOME); L(typeof process.env.PATH); L('PATH' in process.env); L(String(process.env.TERM));",
  "L(J(Object.keys(process.env).filter(function (k) { return /^(BUN|NODE)/.test(k); })));",
  "Bun.env.X2 = 'b'; L(process.env.X2); process.env.X3 = 'p'; L(Bun.env.X3); L(Bun.env === process.env); L(Bun.env.FOO);",
  "L(typeof Bun !== 'undefined' && Bun.env === process.env);"
);

// ---------------------------------------------------------------- 8. argv, cwd, chdir, platform, versão
add(
  "L(Array.isArray(process.argv)); L(process.argv.length); L(process.argv[2]); L(process.argv[3]); L(process.argv[1]); L(process.argv[0] === process.execPath);",
  "L(require('path').isAbsolute(process.argv[0])); L(require('path').basename(process.argv[0])); L(process.argv0);",
  "L(typeof process.argv0); L(process.argv0 === process.argv[0]); L(require('path').basename(process.argv0));",
  "L(J(process.execArgv)); L(Array.isArray(process.execArgv));",
  "L(typeof process.execPath); L(require('path').isAbsolute(process.execPath)); L(require('path').basename(process.execPath)); L(require('fs').existsSync(process.execPath));",
  "L(J(Object.getOwnPropertyDescriptor(process, 'argv'), function (k, v) { return typeof v === 'object' ? 'obj' : v; }));",
  "process.argv.push('extra'); L(process.argv.length); process.argv = ['a']; L(process.argv.length);",
  "L(process.argv.slice(2).join(' '));",
  "L(process.argv[1] === __filename); L(process.argv[1] === require.main.filename);",
  "/*mjs*/ L(process.argv[1] === import.meta.filename); L(process.argv[1]); L(import.meta.path);",
  "L(process.cwd()); L(process.cwd() === __dirname); L(typeof process.cwd); L(process.cwd.length);",
  "process.chdir('/'); L(process.cwd()); process.chdir(__dirname); L(process.cwd());",
  "process.chdir('..'); L(process.cwd() === require('path').dirname(__dirname)); ",
  "try { process.chdir('/nonexistent-dir'); } catch (e) { L(e.name); L(e.code); L(e.message); L(e.syscall); L(e.errno); L(e.path); }",
  "try { process.chdir(); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.chdir(5); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.chdir(__filename); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "L(String(process.chdir('/'))); L(String(process.cwd()));",
  "process.chdir('/'); L(require('path').resolve('x')); L(require('fs').existsSync('bin'));",
  "process.chdir('/'); L(__dirname); L(__filename);",
  "L(process.platform); L(typeof process.platform); L(J(Object.getOwnPropertyDescriptor(process, 'platform')));",
  "L(process.arch); L(typeof process.arch); L(J(Object.getOwnPropertyDescriptor(process, 'arch')));",
  "process.platform = 'win32'; L(process.platform);",
  "process.arch = 'x'; L(process.arch);",
  "L(require('os').platform()); L(require('os').arch()); L(require('os').type());",
  "L(/^v\\d+\\.\\d+\\.\\d+$/.test(process.version)); L(process.version.split('.')[0]); L(J(Object.getOwnPropertyDescriptor(process, 'version')));",
  "L(process.version); ",
  "L(typeof process.versions); L(Object.prototype.toString.call(process.versions)); L(J(Object.keys(process.versions)));",
  "L(J(Object.keys(process.versions).map(function (k) { return typeof process.versions[k]; })));",
  "L(process.versions.node === process.version.slice(1)); L(process.versions.bun); L(typeof process.versions.bun); L(typeof process.versions.v8); L(typeof process.versions.uv); L(typeof process.versions.modules);",
  "L(process.versions.bun === Bun.version); L(J(Object.getOwnPropertyDescriptor(process.versions, 'node')));",
  "L(J(Object.getOwnPropertyDescriptor(process, 'versions'), function (k, v) { return typeof v === 'object' ? 'obj' : v; }));",
  "process.versions.node = '1'; L(process.versions.node === '1'); process.versions.x = 'y'; L(process.versions.x);",
  "L(Object.isFrozen(process.versions)); L(Object.isExtensible(process.versions));",
  "L(typeof process.release); L(J(Object.keys(process.release))); L(process.release.name); L(typeof process.release.sourceUrl); L(typeof process.release.headersUrl);",
  "L(J(Object.getOwnPropertyDescriptor(process, 'release'), function (k, v) { return typeof v === 'object' ? 'obj' : v; }));",
  "L(typeof process.config); L(J(Object.keys(process.config))); L(typeof process.config.variables);",
  "L(typeof process.features); L(J(Object.keys(process.features)));",
  "L(typeof process.title); L(process.title);",
  "process.title = 'renamed'; L(process.title);",
  "L(typeof process.pid); L(Number.isInteger(process.pid)); L(process.pid > 0); L(typeof process.ppid); L(process.ppid > 0);",
  "var d = Object.getOwnPropertyDescriptor(process, 'pid'); L(d.writable + ' ' + d.enumerable + ' ' + d.configurable + ' ' + typeof d.value); process.pid = 1; L(process.pid > 1);",
  "L(typeof process.getuid); L(typeof process.getgid); L(typeof process.geteuid); L(typeof process.getegid); L(typeof process.getgroups);",
  "L(typeof process.umask()); L(process.umask() === process.umask()); L(typeof process.umask(0o22)); L(process.umask(0o22).toString(8)); L(process.umask().toString(8));",
  "L(typeof process.uptime()); L(process.uptime() >= 0); L(process.uptime() < 10); L(process.uptime() <= process.uptime());",
  "var u = process.cpuUsage(); L(J(Object.keys(u))); L(typeof u.user); L(typeof u.system); var d = process.cpuUsage(u); L(d.user >= 0 || d.user < 0);",
  "var r = process.resourceUsage(); L(J(Object.keys(r)));",
  "L(typeof process.stdout); L(typeof process.stderr); L(typeof process.stdin); L(process.stdout.fd); L(process.stderr.fd); L(process.stdin.fd);",
  "L(typeof process.stdout.write); L(process.stdout.write('direct\\n')); L(typeof process.stdout.isTTY); L(String(process.stdout.isTTY)); L(typeof process.stdout.columns);",
  "L(process.stdout.write('')); L(process.stdout.writable); L(process.stdout.constructor.name); L(process.stdout instanceof require('stream').Writable);",
  "var o = process.stdout; L(J(Object.keys(o._events))); L(o._eventsCount); L(String(o._maxListeners)); var f = function () {}; o.on('foo', f); L(J(Object.keys(o._events))); L(o._eventsCount); L(o._events.foo === f); o.on('foo', function () {}); L(Array.isArray(o._events.foo)); L(o._events.foo.length); L(o._eventsCount);",
  "var o = process.stdout; var f = function () {}; o.once('bar', f); L(typeof o._events.bar); L(o._events.bar.name); L(o._events.bar.listener === f); L(o._eventsCount); o.removeAllListeners('bar'); L(J(Object.keys(o._events))); L(o._eventsCount);",
  "var o = process.stdout; o.on('zed', function () {}); o.removeAllListeners(); L(J(Object.keys(o._events))); L(o._eventsCount); L(J(o.eventNames()));",
  "var o = process.stdout; o.on('newListener', function (e, l) { L('newListener ' + e + ' ' + typeof l); }); o.on('zed', function () {}); o.once('zed2', function () {}); L(J(Object.keys(o._events))); L(o._eventsCount); o.removeAllListeners();",
  "var o = process.stdout; L(String(o._maxListeners)); L(o.setMaxListeners(5) === o); L(o._maxListeners); L(o.getMaxListeners()); L(J(Object.keys(o).slice(-2)));",
  "var o = process.stdout; L(o._writableState.pendingcb); o.write('', function () {}); L(o._writableState.pendingcb); L(o._writableState.onwrite.name); L(o._writableState.onwrite.length); try { o._writableState.onwrite(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } L(String(o._writableState.onwrite(1))); L(o._writableState.pendingcb);",
  "var WS = process.stdout.constructor; try { WS(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } try { new WS(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); }",
  "var W = Object.getPrototypeOf(process.stdout.constructor.prototype).constructor; L(W.name); var x = W(); L(J(Object.keys(x))); L(J(Object.keys(x._events))); L(J(Object.keys(x._writableState))); var y = new W(); L(J(Object.keys(y)));",
  "var S = Object.getPrototypeOf(Object.getPrototypeOf(process.stdout.constructor.prototype)).constructor; L(S.name); try { S(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } var y = new S(); L(J(Object.keys(y))); L(y._eventsCount);",
  "var E = require('events'); try { E(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } var y = new E(); L(J(Object.keys(y))); L(J(Object.keys(y._events))); L(y._eventsCount); L(String(y._maxListeners));",
  "var R = Object.getPrototypeOf(process.stdin.constructor.prototype).constructor; L(R.name); var a = R(); var b = new R(); L(J(Object.keys(a))); L(J(Object.keys(a._events))); L(J(Object.keys(a._readableState))); L(J(Object.keys(b))); L(a instanceof R); L(Object.getPrototypeOf(a._events) === null);",
  "var RS = process.stdin.constructor; L(RS.name); try { RS(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } try { new RS(); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } try { RS(1); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); }",
  "var r = process.stdin.constructor('/etc/hostname'); L(J(Object.keys(r))); L(r.fd); L(r.path); L(r.flags); L(r.mode); L(String(r.end)); L(r.bytesRead); L(J(Object.keys(r._events))); L(r._eventsCount);",
  "var WS = process.stdout.constructor; var w = WS('/dev/null'); L(J(Object.keys(w))); L(w.fd); L(w.path); L(w.flags); L(w.mode); L(w.bytesWritten); L(J(Object.keys(w._events))); L(w._eventsCount); L(WS('/dev/null', { flags: 'a' }).flags); L(w instanceof WS);",
  "var o = process.stdout; var e = o._events; L(Object.getPrototypeOf(e) === null); var f = function () {}; o.on('x', f); L(o._events === e); o.off('x', f); L(o._events === e); L(J(Object.keys(o._events))); L(String(o._events.x)); L(o._eventsCount); o.removeAllListeners(); L(o._events === e);",
  "var i = process.stdin; L(J(Object.keys(i))); L(J(Object.getOwnPropertyNames(i))); L(String(i.start)); L(i.end); L(String(i.pos)); L(i.bytesRead); L(String(i._maxListeners)); L(i._eventsCount); L(J(Object.keys(i._events))); L(J(Object.keys(i._readableState)));",
  "var i = process.stdin; ['close', 'end', 'resume', 'pause'].forEach(function (k) { var f = i._events[k]; L(k + ' ' + JSON.stringify(f.name) + '/' + f.length + ' ' + f.toString() + ' ' + i.listeners(k).length + ' ' + (i.listeners(k)[0] === f)); }); L(i._eventsCount); L(i.listenerCount('data'));",
  "var i = process.stdin; i.on('data', function () {}); L(i._eventsCount); i.on('readable', function () {}); L(i._eventsCount); L(J(Object.keys(i._events).filter(function (k) { return i._events[k] !== undefined; })));",
  "var i = process.stdin; i.setEncoding('utf8'); var log = []; i.on('data', function (d) { log.push('data:' + JSON.stringify(d)); }); i.on('end', function () { log.push('end'); }); i.on('close', function () { log.push('close'); }); setTimeout(function () { L(log.join('|')); }, 50);",
  "var S = require('stream'); var st = new S(); var w = S.Writable(); st.on('x', function () {}); w.on('y', function () {}); L(st._eventsCount); L(J(Object.keys(st._events))); L(J(Object.keys(w._events))); L(process.stdout.listenerCount('x') + ' ' + process.stdout.listenerCount('y') + ' ' + st.listenerCount('x') + ' ' + w.listenerCount('y')); L(process.stdout._eventsCount);",
  "var i = process.stdin; ['on', 'addListener', 'pause', 'resume', 'read', '_read'].forEach(function (k) { var d = Object.getOwnPropertyDescriptor(i, k); L(k + ' ' + i[k].name + '/' + i[k].length + ' ' + d.enumerable + d.writable + d.configurable); }); L(i.pause() === i); L(i.resume() === i); L(String(i.read())); L(String(i._read(1))); L(Object.getPrototypeOf(i._events) === null);",
  "var e = process.stderr; var r = e.end('a\\n', function () { L('cb1 ' + arguments.length + ' ' + arguments[0]); }); L(r === e); L(e.writableFinished); e.end(function () { L('cb2 ' + arguments.length + ' ' + arguments[0]); }); e.on('finish', function () { L('finish'); }); process.nextTick(function () { L('tick ' + e.writableFinished); });",
  "var e = process.stderr; e.end(); setTimeout(function () { var r = e.end(function (er) { L(er.code + ' ' + er.message); }); L(r === e); L(e.end() === e); }, 20);",
  "var i = process.stdin; L(i.isPaused()); L(String(i.readableFlowing)); i.on('data', function () {}); L(i.isPaused() + ' ' + i.readableFlowing); i.pause(); L(i.isPaused() + ' ' + i.readableFlowing); i.resume(); L(i.isPaused() + ' ' + i.readableFlowing);",
  "var i = process.stdin; i.on('readable', function () {}); L(i.isPaused() + ' ' + i.readableFlowing); i.resume(); L(i.isPaused() + ' ' + i.readableFlowing); i.pause(); L(i.isPaused() + ' ' + i.readableFlowing); i.on('data', function () {}); L(i.isPaused() + ' ' + i.readableFlowing);",
  "var i = process.stdin; i.pause(); i.on('data', function () {}); L(i.isPaused() + ' ' + i.readableFlowing);",
  "var i = process.stdin; i.on('data', function () {}); L(i._eventsCount); process.nextTick(function () { L('tick ' + i._eventsCount); }); Promise.resolve().then(function () { L('micro ' + i._eventsCount); });",
  "var i = process.stdin; L(i._eventsCount); process.nextTick(function () { L('tick ' + i._eventsCount); }); Promise.resolve().then(function () { L('micro ' + i._eventsCount); });",
  "var R = Object.getPrototypeOf(Object.getPrototypeOf(process.stdin)); var p = Object.getOwnPropertyDescriptor(R, 'readableFlowing'); L(p.get.name + '/' + p.get.length + ' ' + p.set.name + '/' + p.set.length + ' ' + p.enumerable + p.configurable); L(R.isPaused.call({ _readableState: { flowing: false } })); L(R.isPaused.call({ _readableState: { paused: true } })); L(p.get.call({ _readableState: { flowing: true } }));",
  "var R = Object.getPrototypeOf(Object.getPrototypeOf(process.stdin)); var p = Object.getOwnPropertyDescriptor(R, 'readableFlowing'); try { R.isPaused.call({}); } catch (e) { L(e.name + ' ' + e.message); } try { p.get.call({}); } catch (e) { L(e.name + ' ' + e.message); } var x = {}; L(String(p.set.call(x, true))); L(J(Object.keys(x)));",
  "var i = process.stdin; i.on('data', function () {}); i.pause(); L(i._readableState.flowing + ' ' + i.isPaused()); i.readableFlowing = true; L(J(Object.keys(i).indexOf('readableFlowing')) + ' ' + i._readableState.flowing + ' ' + i.readableFlowing + ' ' + i.isPaused());",
  "var i = process.stdin; var ev = i._events; var r = []; ['close', 'end', 'resume', 'pause'].forEach(function (k) { r.push(k + ':' + String(ev[k].call(i, 1, 2)) + ' ' + i._readableState.flowing + ' ' + i.isPaused() + ' ' + i._eventsCount); }); L(r.join('|'));",
  "var i = process.stdin; i.on('data', function () {}); var ev = i._events; ev.pause.call(i); L(i.readableFlowing + ' ' + i.isPaused()); ev.resume.call(i); L(i.readableFlowing + ' ' + i.isPaused()); ev.end.call(i); ev.close.call(i); L(i.destroyed + ' ' + i.readable + ' ' + i.readableFlowing);",
  "var S = require('stream'); var a = new S(); var b = new S(); var w = S.Writable(); a.on('x', function () {}); a.on('y', function () {}); L(a._eventsCount + ' ' + b._eventsCount + ' ' + w._eventsCount); L(J(a.eventNames())); L(J(b.eventNames())); L(J(process.stdout.eventNames())); L(a.listenerCount('x') + ' ' + b.listenerCount('x')); L(a.emit('x') + ' ' + b.emit('x')); a.removeAllListeners(); L(a._eventsCount + ' ' + J(Object.keys(a._events)));",
  "var e = process.stderr; e.end(); e.end('', function (er) { L(er.code + ' ' + er.message); });",
  "L(typeof process.binding); L(typeof process.dlopen); L(typeof process.allowedNodeEnvironmentFlags); L(typeof process.report); L(typeof process.debugPort); L(typeof process.sourceMapsEnabled);",
  "L(typeof process.kill); try { process.kill(process.pid, 0); L('alive'); } catch (e) { L(e.code); }",
  "try { process.kill(999999, 0); } catch (e) { L(e.name); L(e.code); L(e.syscall); L(e.message); }",
  "try { process.kill(process.pid, 'SIGNOPE'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "process.on('SIGUSR2', function (s, n) { L('got ' + s + ' ' + n); process.exit(); }); process.kill(process.pid, 'SIGUSR2'); setTimeout(function () { L('no signal'); }, 50);",
  "L(process.listenerCount('SIGUSR2')); process.on('SIGUSR2', function () {}); L(process.listenerCount('SIGUSR2')); process.removeAllListeners('SIGUSR2'); L(process.listenerCount('SIGUSR2'));",
  "process.on('SIGTERM', function () { L('term'); process.exit(0); }); process.kill(process.pid, 'SIGTERM'); setTimeout(function () {}, 50);",
  "process.kill(process.pid, 'SIGTERM'); setTimeout(function () { L('survived'); }, 50);",
  "process.on('exit', function (c) { L('exit ' + c); }); process.kill(process.pid, 'SIGTERM'); setTimeout(function () {}, 50);",
  "process.abort();",
  "process.on('exit', function () { L('exit'); }); process.abort();",
  "var i = process.stdin; ['utf8', 'UTF-8', 'hex', 'base64', 'base64url', 'latin1', 'binary', 'ascii', 'ucs2', 'UCS-2', 'utf16le', 'Hex'].forEach(function (n) { L(n + ' ' + (i.setEncoding(n) === i)); });",
  "var i = process.stdin; [undefined, null].forEach(function (n) { L(String(n) + ' ' + (i.setEncoding(n) === i)); }); ['nope', 'utf-16', '', 5, {}].forEach(function (n) { try { i.setEncoding(n); L('ok'); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } });",
  "var i = process.stdin; i.setEncoding('base64'); var log = []; ['data', 'end', 'close'].forEach(function (k) { i.on(k, function (d) { log.push(k + (d === undefined ? '' : ':' + typeof d)); }); }); setTimeout(function () { L(log.join('|')); }, 50);",
  "var i = process.stdin; i.setEncoding('utf16le'); i.setEncoding('hex'); i.on('data', function (d) { L('data ' + typeof d); }); i.on('end', function () { L('end ' + i.readableFlowing); }); setTimeout(function () { L('done'); }, 50);",
  "var i = process.stdin; i.on('end', function () { i.setEncoding('base64'); L('late ' + (i.readableFlowing)); }); i.on('data', function () {}); setTimeout(function () { L('done'); }, 50);"
);

// ---------------------------------------------------------------- 9. hrtime, memoryUsage, uptime
add(
  "var t = process.hrtime(); L(Array.isArray(t)); L(t.length); L(typeof t[0]); L(typeof t[1]); L(Number.isInteger(t[0])); L(Number.isInteger(t[1])); L(t[1] >= 0 && t[1] < 1e9); L(t[0] >= 0);",
  "var t = process.hrtime(); var d = process.hrtime(t); L(Array.isArray(d)); L(d.length); L(d[0] >= 0); L(d[1] >= 0 && d[1] < 1e9); L(d[0] === 0);",
  "var t = process.hrtime(); var u = process.hrtime(); L(u[0] > t[0] || (u[0] === t[0] && u[1] >= t[1]));",
  "var t = process.hrtime([1, 500000000]); L(J(t.map(function (x) { return typeof x; }))); L(t[0] > 1000);",
  "var d = process.hrtime([1e9, 0]); L(d[0] < 0 || d[0] >= 0); L(d[1] >= 0 && d[1] < 1e9);",
  "var b = process.hrtime.bigint(); L(typeof b); L(b > 0n); var c = process.hrtime.bigint(); L(c >= b);",
  "var t = process.hrtime(); var b = process.hrtime.bigint(); var diff = b - BigInt(t[0]) * 1000000000n - BigInt(t[1]); L(diff >= 0n); L(diff < 1000000000n);",
  "var b0 = process.hrtime.bigint(); var s = Date.now(); while (Date.now() - s < 10); var b1 = process.hrtime.bigint(); L(b1 - b0 >= 9000000n); L(b1 - b0 < 500000000n);",
  "var t0 = process.hrtime(); var s = Date.now(); while (Date.now() - s < 10); var d = process.hrtime(t0); L(d[0] === 0 && d[1] >= 9000000);",
  "var p = performance.now(); var h = Number(process.hrtime.bigint()) / 1e6; L(Math.abs((h - performance.timeOrigin) - p) >= 0);",
  "try { process.hrtime(1); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime('x'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime({}); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime([1]); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime([1, 2, 3]); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime(['a', 'b']); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime(null); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime(undefined); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime([-1, 0]); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime([1.5, 0.5]); L('no-throw'); } catch (e) { L(e.name); L(e.code); L(e.message); }",
  "try { process.hrtime.bigint(1); L('no-throw'); } catch (e) { L(e.name); }",
  "try { process.hrtime.bigint.call(null); L('no-throw'); } catch (e) { L(e.name); }",
  "var h = process.hrtime; L(Array.isArray(h())); var hb = process.hrtime.bigint; L(typeof hb());",
  "var m = process.memoryUsage(); L(J(Object.keys(m))); L(J(Object.keys(m).map(function (k) { return typeof m[k]; }))); L(Object.keys(m).every(function (k) { return Number.isInteger(m[k]) && m[k] > 0; }));",
  "var m = process.memoryUsage(); L(m.rss > m.heapUsed); L(m.heapTotal >= m.heapUsed); L(Object.getPrototypeOf(m) === Object.prototype); L(m.rss > 1e6);",
  "var m = process.memoryUsage(); L(m.arrayBuffers >= 0); L(m.external >= 0);",
  "var m = process.memoryUsage(); var a = new ArrayBuffer(50 * 1024 * 1024); var n = process.memoryUsage(); L(n.arrayBuffers - m.arrayBuffers >= 50 * 1024 * 1024);",
  "var r = process.memoryUsage.rss(); L(typeof r); L(Number.isInteger(r)); L(r > 1e6);",
  "L(typeof process.memoryUsage.rss()); L(process.memoryUsage.rss.length);",
  "var d = Object.getOwnPropertyDescriptor(process.memoryUsage(), 'rss'); L(d.writable + ' ' + d.enumerable + ' ' + d.configurable + ' ' + typeof d.value);",
  "var m = process.memoryUsage(); m.rss = 1; L(process.memoryUsage().rss !== 1);",
  "L(process.memoryUsage() !== process.memoryUsage());",
  "L(typeof process.availableMemory); L(typeof process.constrainedMemory);",
  "var u = process.uptime(); var s = Date.now(); while (Date.now() - s < 10); L(process.uptime() - u >= 0.009);",
  "L(Number.isFinite(process.uptime())); L(process.uptime() !== Math.floor(process.uptime()));",
  "L(process.hrtime.bigint() > BigInt(Math.floor(process.uptime() * 1e9)));",
  "L(typeof performance.now()); L(performance.now() < 10000); L(performance.now() > 0);"
);

// ---------------------------------------------------------------- 10. mais nextTick e stdio entrelaçados
add(
  "process.stdout.write('a\\n'); process.nextTick(function () { process.stdout.write('b\\n'); }); console.log('c');",
  "process.nextTick(function () { console.log('tick'); }); process.stdout.write('write\\n', function () { L('write cb'); });",
  "process.stdout.write('x', function () { L('cb1'); }); process.nextTick(function () { L('tick'); }); Promise.resolve().then(function () { L('then'); });",
  "process.stdout.write('data\\n', 'utf8', function () { L('cb'); });",
  "process.on('exit', function () { process.stdout.write('exit write\\n'); }); process.nextTick(function () { L('tick'); });",
  "console.error('to stderr'); process.nextTick(function () { console.error('tick stderr'); });",
  "process.stderr.write('direct stderr\\n'); L('out');",
  "process.nextTick(function () { process.stdout.write('tick write\\n'); process.exit(3); });",
  "process.stdout.write('before exit\\n'); process.exit(4);",
  "var big = 'x'.repeat(200000); process.stdout.write(big + '\\n'); process.exit(0);",
  "console.log('a'); process.exit(); console.log('b');",
  "process.nextTick(process.exit, 6);",
  "process.nextTick(process.exit);",
  "process.nextTick(function () { process.exitCode = 5; }); ",
  "process.nextTick(function () { process.exitCode = 5; }); process.on('exit', function (c) { L('exit ' + c); });",
  "setTimeout(process.exit, 1, 8);",
  "setImmediate(process.exit, 2);",
  "Promise.resolve().then(function () { process.exit(9); }); process.nextTick(function () { L('tick'); });",
  "process.nextTick(function () { L('tick1'); process.exit(1); L('never'); }); process.nextTick(function () { L('tick2'); });",
  "process.on('exit', function () { L('exit'); }); process.nextTick(function () { throw new Error('tick throws'); });",
);

// ---------------------------------------------------------------- 11. ordem e falhas de monitor, captura e ouvintes (fatia 4, medidos)
// Sempre no fim do arquivo: acrescentar aqui não desloca os índices das seções anteriores.
add(
  "process.on('uncaughtExceptionMonitor', function (e, o) { L('monitor ' + o); }); process.setUncaughtExceptionCaptureCallback(function (e) { L('capture ' + e.message); }); process.on('uncaughtException', function () { L('handler'); }); throw new Error('boom');",
  "process.on('uncaughtExceptionMonitor', function (e) { L('monitor'); process.exit(4); }); process.on('uncaughtException', function () { L('handler'); }); throw new Error('boom');",
  "process.on('uncaughtExceptionMonitor', function (e) { L('monitor'); throw new Error('from monitor'); }); process.on('uncaughtException', function (e) { L('handler ' + e.message); }); throw new Error('boom');",
  "process.on('uncaughtExceptionMonitor', function (e) { L('monitor'); throw new Error('from monitor'); }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c); }); process.on('uncaughtException', function () { L('handler'); throw new Error('from handler'); }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c + ' ' + process.exitCode); }); process.on('unhandledRejection', function () { L('handler'); throw new Error('from handler'); }); Promise.reject(new Error('rej'));",
  "process.on('exit', function (c) { L('exit ' + c); }); process.on('uncaughtException', function (e) { L('uncaught ' + e.message); }); process.on('unhandledRejection', function () { L('handler'); throw new Error('from handler'); }); Promise.reject(new Error('rej'));",
  "process.on('exit', function (c) { L('exit ' + c); }); process.setUncaughtExceptionCaptureCallback(function () { L('capture'); throw new Error('from capture'); }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c); }); process.on('uncaughtException', function () { L('handler'); process.exit(5); }); throw new Error('boom');",
  "process.on('uncaughtException', function () { L('handler'); process.exit(); }); throw new Error('boom');",
  "process.exitCode = 3; process.on('uncaughtException', function () { L('handler'); process.exit(); }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c); }); process.on('uncaughtException', function () { L('handler'); process.exitCode = 9; }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c); }); process.setUncaughtExceptionCaptureCallback(function () { L('capture'); process.exit(6); }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c); }); process.on('uncaughtException', function (e) { L('first'); }); process.on('uncaughtException', function (e) { L('second'); throw new Error('x'); }); throw new Error('boom');",
  "process.on('uncaughtException', function () { L('handler'); throw new Error('from handler'); }); process.on('uncaughtException', function () { L('second'); }); throw new Error('boom');",
  "process.on('exit', function (c) { L('exit ' + c); }); setTimeout(function () { L('timer'); }, 5); process.on('uncaughtException', function () { L('handler'); throw new Error('from handler'); }); throw new Error('boom');",
  "process.on('uncaughtExceptionMonitor', function (e, o) { L('monitor ' + o); process.exit(2); }); Promise.reject(new Error('rej'));",
  "process.on('exit', function (c) { L('exit ' + c); }); process.on('uncaughtExceptionMonitor', function () { L('monitor'); }); process.on('uncaughtException', function () { L('handler'); throw new Error('from handler'); }); throw new Error('boom');",
  "var vs = [5, {}, null, undefined, Symbol('x'), [1, 2], ['utf8'], { toString: function () { throw new Error('boom'); } }, true, 1.5, 10n, Object.create(null), '']; vs.forEach(function (v, k) { try { process.stdin.setEncoding(v); L(k + ' ok'); } catch (e) { L(k + ' ' + e.constructor.name + ' ' + e.name + ' ' + e.code + ' ' + e.message); } });",
  "var vs = [5, {}, null, undefined, Symbol('x'), [1, 2], ['utf8'], { toString: function () { throw new Error('boom'); } }, true, 1.5, 10n, Object.create(null), '', function f() {}]; ['stdout', 'stderr'].forEach(function (s) { vs.forEach(function (v, k) { try { process[s].setDefaultEncoding(v); L(s + k + ' ok'); } catch (e) { L(s + k + ' ' + e.constructor.name + ' ' + e.name + ' ' + e.code + ' ' + e.message); } }); });",
  "try { process.stdout.setDefaultEncoding(); L('ok'); } catch (e) { L(e.name + ' ' + e.code + ' ' + e.message); } try { L(process.stdout.setDefaultEncoding('HEX') === process.stdout); } catch (e) { L(e.message); }"
);

// ---------------------------------------------------------------- execução
const round = () => programs.map((p) => runChild(p));
const rounds = [];
for (let i = 0; i < REPEAT; i++) rounds.push(round());
const rows = [];
let dropped = 0;
programs.forEach((program, index) => {
  const result = rounds[0][index];
  if (rounds.every((r) => r[index] === result)) rows.push({ source: PRELUDE + program, result });
  else {
    dropped++;
    console.error("instável: " + program + " => " + rounds.map((r) => JSON.stringify(r[index])).join(" | "));
  }
});
console.error("casos mantidos: " + rows.length + ", descartados: " + dropped);
process.stdout.write(emitFactored("process", rows));
