// Gera tests/golden/queue_microtask_bun.tsv: o `queueMicrotask` do global medido no bun 1.4.2. O JavaScriptCore não o
// define (quem instala é o bun, pelo lado do WebCore), então o golden fixa o descritor, `name`, `length`, `toString`,
// a mensagem do erro com argumento que não é função, e a ordem relativa a `Promise.resolve().then`.
// Formato (scripts/golden-prelude.js, mesmo prelúdio de microtask_order): o log é a string global `R`, lida depois de
// esvaziadas as microtarefas. Cada programa roda como arquivo num bun próprio, com o próprio `queue_microtask.preludes.json`.
// Uso: bun scripts/gen-queue-microtask-golden.js > tests/golden/queue_microtask_bun.tsv
const { emitFactoredLines } = require("./golden-prelude.js");
const { asyncGolden, ORDER_HARNESS } = require("./async-golden.js");

// O programa é um arquivo (o bun transpila o fonte quando roda arquivo): o prelúdio de microtask_order e o corpo, num bun
// novo por programa. Erro lançado dentro de uma microtarefa vai ao `uncaughtException` do preload, que o engole, e as
// microtarefas seguintes rodam. O arquivo se chama `queue_microtask_case.js` dos dois lados.
const { measureBodies } = asyncGolden({ harness: ORDER_HARNESS, catchErrors: false });

const ERR = (arg) => `try { queueMicrotask(${arg}); L('no-throw'); } catch (e) { L(e.name + ':' + e.message + ':' + (e instanceof TypeError)); }`;
const programs = [
  "L(typeof globalThis.queueMicrotask);",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'queueMicrotask'), (k, v) => typeof v === 'function' ? 'fn' : v));",
  "L(queueMicrotask.name); L(queueMicrotask.length); L(Function.prototype.toString.call(queueMicrotask));",
  "L(queueMicrotask.hasOwnProperty('prototype')); L(Object.getPrototypeOf(queueMicrotask) === Function.prototype);",
  "L(String(queueMicrotask(() => {})));",
  "queueMicrotask(() => L('q')); L('sync');",
  "Promise.resolve().then(() => L('p1')); queueMicrotask(() => L('q')); Promise.resolve().then(() => L('p2'));",
  "queueMicrotask(() => L('q1')); queueMicrotask(() => L('q2')); Promise.resolve().then(() => L('p')); queueMicrotask(() => L('q3'));",
  "queueMicrotask(() => { L('q1'); queueMicrotask(() => L('q-inner')); Promise.resolve().then(() => L('p-inner')); }); queueMicrotask(() => L('q2'));",
  "Promise.resolve().then(() => { L('p1'); queueMicrotask(() => L('q-in-p')); }).then(() => L('p2'));",
  "queueMicrotask(function () { L(typeof this); L(this === globalThis); L(arguments.length); });",
  "queueMicrotask((...a) => L(a.length), 1, 2, 3);",
  "(async () => { L('a1'); await null; L('a2'); })(); queueMicrotask(() => L('q')); tick(2, 't2');",
  "queueMicrotask(() => L('q')); (async () => { L('a1'); await null; L('a2'); })(); tick(2, 't2');",
  ERR("undefined"),
  ERR("1"),
  ERR("'x'"),
  ERR("{}"),
  ERR("null"),
  ERR("true"),
  ERR("[]"),
  ERR("Symbol()"),
  "try { queueMicrotask(); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }",
  "try { new queueMicrotask(() => {}); L('no-throw'); } catch (e) { L(e.name); }",
  "var f = queueMicrotask; f.call(undefined, () => L('called')); f.apply(null, [() => L('applied')]); f.bind(1)(() => L('bound'));",
  "var q = queueMicrotask; queueMicrotask = 5; L(typeof queueMicrotask); queueMicrotask = q; L(typeof queueMicrotask);",
  "L(delete globalThis.queueMicrotask); L(typeof globalThis.queueMicrotask);",
  "L(Object.keys(globalThis).includes('queueMicrotask')); L(Object.getOwnPropertyNames(globalThis).includes('queueMicrotask'));",
  // A posição de `queueMicrotask` em getOwnPropertyNames(globalThis) não entra: o caso de `delete` acima o
  // recoloca no fim ao restaurá-lo, então a ordem original não é mais observável neste processo.
  // Callback que lança: o erro vai ao uncaughtException (o harness o engole) e as microtasks seguintes rodam.
  "queueMicrotask(() => { L('q1'); throw new Error('boom'); }); queueMicrotask(() => L('q2')); Promise.resolve().then(() => L('p'));",
  "queueMicrotask(() => { throw 5; }); queueMicrotask(() => L('q2'));",
  "queueMicrotask(() => { L('q1'); queueMicrotask(() => L('q-inner')); throw new TypeError('x'); }); queueMicrotask(() => L('q2')); Promise.resolve().then(() => L('p'));",
  "queueMicrotask(() => { throw new Error('boom'); }); Promise.resolve().then(() => L('p1')).then(() => L('p2')); (async () => { await null; L('a'); })();",
];

measureBodies(programs, "queue_microtask_case.js", { swallowUncaught: true }).then((lines) => {
  process.stdout.write(emitFactoredLines("queue_microtask", lines));
});
