// Gera tests/golden/timers_bun.tsv: setTimeout, setInterval, setImmediate e os três clear* medidos no bun 1.4.2.
// Formato idêntico ao do queue_microtask (scripts/gen-queue-microtask-golden.js): o log é a string global `R`, lida
// depois que o laço de eventos esvaziou. Cada programa leva um timer-sentinela de 30 ms (o laço fica vivo até lá,
// nos dois lados), então todos os atrasos dos casos são menores que 30. Os ids absolutos dependem do histórico do
// processo, por isso os casos só observam diferenças de id.
// O bun mede em tempo real; para não gravar ruído, a geração roda o conjunto inteiro REPEAT vezes e só mantém as
// linhas em que todas as rodadas concordam (as descartadas vão para o stderr).
// Uso: bun scripts/gen-timers-golden.js > tests/golden/timers_bun.tsv
const fs = require("fs");
const path = require("path");

const prelude = fs.readFileSync(path.join(__dirname, "..", "tests", "golden", "microtask_order.preludes.json"), "utf8");
const REPEAT = 3;
const SENTINEL = "setTimeout(function () {}, 30); ";

const programs = [];
const add = (source) => programs.push(source);

// ---------------------------------------------------------------- forma dos globais
const FUNCTIONS = ["setTimeout", "setInterval", "setImmediate", "clearTimeout", "clearInterval", "clearImmediate"];
for (const name of FUNCTIONS) {
  add(`L(typeof globalThis.${name});`);
  add(`L(JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, '${name}'), (k, v) => typeof v === 'function' ? 'fn' : v));`);
  add(`L(${name}.name); L(${name}.length); L(Function.prototype.toString.call(${name}));`);
  add(`L(${name}.hasOwnProperty('prototype')); L(Object.getPrototypeOf(${name}) === Function.prototype);`);
  add(`L(Object.keys(globalThis).includes('${name}')); L(Object.getOwnPropertyNames(globalThis).includes('${name}'));`);
  add(`try { new ${name}(() => {}); L('no-throw'); } catch (e) { L(e.name); }`);
}
add(
  "var set = ['clearImmediate','clearInterval','clearTimeout','queueMicrotask','setImmediate','setInterval','setTimeout'];" +
    "L(Object.getOwnPropertyNames(globalThis).filter(n => set.includes(n)).join(','));" +
    "L(Object.keys(globalThis).filter(n => set.includes(n)).join(','));",
);
add("var s = setTimeout; setTimeout = 5; L(typeof setTimeout); setTimeout = s; L(typeof setTimeout);");

// ---------------------------------------------------------------- forma dos objetos devolvidos
const MAKERS = { Timeout: "setTimeout(() => {}, 1)", Interval: "setInterval(() => {}, 1)", Immediate: "setImmediate(() => {})" };
const CLEAR_OF = { Timeout: "clearTimeout", Interval: "clearInterval", Immediate: "clearImmediate" };
for (const [kind, make] of Object.entries(MAKERS)) {
  const pre = `var t = ${make}; var P = Object.getPrototypeOf(t); `;
  const post = ` ${CLEAR_OF[kind]}(t);`;
  add(pre + "L(typeof t); L(t.constructor.name); L(Object.keys(t).length); L(Object.getOwnPropertyNames(t).length); L(Reflect.ownKeys(t).length);" + post);
  add(pre + "L(Reflect.ownKeys(P).map(String).join(','));" + post);
  add(pre + "L(Object.getPrototypeOf(P) === Object.prototype); L(P.constructor.name); L(P.constructor.length); L(Function.prototype.toString.call(P.constructor));" + post);
  add(pre + "var C = P.constructor; L(typeof C); L(C.length); L(C.name); L(typeof C.prototype); L(Object.getOwnPropertyNames(C).join(','));" + post);
  add(pre + "try { new P.constructor(); L('no-throw'); } catch (e) { L(e.name + ':' + e.message + ':' + e.code + ':' + (e instanceof TypeError)); }" + post);
  add(pre + "try { P.constructor(); L('no-throw'); } catch (e) { L(e.name + ':' + e.message + ':' + e.code); }" + post);
  add(pre + "try { Reflect.construct(P.constructor, []); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }" + post);
  add(pre + "L(Object.prototype.toString.call(t)); L(JSON.stringify(t)); L(typeof P[Symbol.toPrimitive]); L(P[Symbol.toStringTag]);" + post);
  add(
    pre +
      "Reflect.ownKeys(P).forEach(k => { var d = Object.getOwnPropertyDescriptor(P, k); L(String(k) + '=' + (d.get ? 'acc:' + d.get.name + '/' + (d.set ? d.set.name : '-') : typeof d.value + ':' + (typeof d.value === 'function' ? d.value.length + ':' + d.value.name : '')) + (d.enumerable ? 'E' : 'e') + (d.configurable ? 'C' : 'c') + (d.writable ? 'W' : 'w')); });" +
      post,
  );
  add(pre + "L(String(t) === String(+t)); L(typeof (+t)); L(Number.isInteger(+t)); L(`${t}` === String(t)); L(typeof t[Symbol.toPrimitive]('string'));" + post);
  add(pre + "L(t.hasRef()); L(t.unref() === t); L(t.hasRef()); L(t.ref() === t); L(t.hasRef());" + post);
  add(pre + "L(t._destroyed);" + post + " L(t._destroyed); L(t.hasRef());");
  add(pre + "L(t[Symbol.dispose] === P[Symbol.dispose]); t[Symbol.dispose](); L(t._destroyed);");
  add(`var a = ${make}; var b = ${make}; var c = ${make}; L((+b - +a) + ',' + (+c - +b)); ${CLEAR_OF[kind]}(a); ${CLEAR_OF[kind]}(b); ${CLEAR_OF[kind]}(c);`);
}
add("var a = setTimeout(() => {}, 1), b = setInterval(() => {}, 1), c = setImmediate(() => {}), d = setTimeout(() => {}, 1); L([+b - +a, +c - +b, +d - +c].join()); clearTimeout(a); clearInterval(b); clearImmediate(c); clearTimeout(d);");
for (const [kind, make] of Object.entries(MAKERS)) {
  if (kind === "Immediate") continue;
  const pre = `var t = ${make}; `;
  const post = ` ${CLEAR_OF[kind]}(t);`;
  add(pre + "L(t._destroyed + ':' + t._idleTimeout + ':' + typeof t._onTimeout + ':' + t._repeat + ':' + typeof t._idleStart);" + post);
  add(pre + "t._idleTimeout = 50; L(t._idleTimeout); t._onTimeout = 5; L(t._onTimeout); t._repeat = 7; L(t._repeat); t._destroyed = true; L(t._destroyed);" + post);
  add(pre + "L(t.close() === t); L(t._destroyed); L(t.hasRef());");
  add(pre + "L(t.refresh() === t); L(t._destroyed);" + post);
}

// ---------------------------------------------------------------- erros de argumento
const BAD_ARGS = ["", "undefined", "null", "1", "'x'", "{}", "[]", "true", "Symbol()", "1n", "'function'", "/x/", "new Date(0)"];
for (const name of ["setTimeout", "setInterval", "setImmediate"]) {
  for (const arg of BAD_ARGS) {
    add(`try { ${name}(${arg}); L('no-throw'); } catch (e) { L(e.name + ':' + e.message + ':' + (e instanceof TypeError) + ':' + Object.getOwnPropertyNames(e).includes('code')); }`);
  }
  const clearer = { setTimeout: "clearTimeout", setInterval: "clearInterval", setImmediate: "clearImmediate" }[name];
  add(`try { var h1 = ${name}.call(undefined, () => { L('called'); ${clearer}(h1); }); var h2 = ${name}.call(null, () => { L('called2'); ${clearer}(h2); }); var h3 = ${name}.apply(1, [() => { L('applied'); ${clearer}(h3); }]); } catch (e) { L(e.name); }`);
}
add("try { setTimeout(() => {}, Symbol()); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }");
add("try { setInterval(() => {}, Symbol()); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }");
add("try { setTimeout(() => {}, 1n); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }");
add("try { setImmediate(() => L('i'), Symbol()); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }");
add("try { setTimeout(() => {}, { valueOf() { throw new RangeError('vo'); } }); L('no-throw'); } catch (e) { L(e.name + ':' + e.message); }");
for (const name of ["clearTimeout", "clearInterval", "clearImmediate"]) {
  for (const arg of ["", "undefined", "null", "1", "'x'", "{}", "[]", "true", "Symbol()", "1n", "-1", "NaN", "1e9", "1.5"]) {
    add(`try { L(String(${name}(${arg}))); } catch (e) { L(e.name + ':' + e.message); }`);
  }
}

// ---------------------------------------------------------------- normalização do atraso
const DELAYS = ["undefined", "null", "0", "0.5", "1", "1.5", "2.7", "'5'", "-3", "NaN", "true", "false", "'abc'", "''", "2147483647", "2147483648", "Infinity", "-Infinity", "1e10", "({ valueOf() { return 3; } })", "[7]", "'  9  '", "0x10", "1e1", "-0"];
for (const d of DELAYS) {
  add(`var t = setTimeout(() => {}, ${d}); L(t._idleTimeout); clearTimeout(t);`);
  add(`var t = setInterval(() => {}, ${d}); L(t._idleTimeout + ':' + t._repeat); clearInterval(t);`);
}

// ---------------------------------------------------------------- ordem: todas as sequências de 3 itens
const ITEMS = {
  T0: (n) => `setTimeout(() => L('${n}'), 0);`,
  T10: (n) => `setTimeout(() => L('${n}'), 10);`,
  T20: (n) => `setTimeout(() => L('${n}'), 20);`,
  I: (n) => `setImmediate(() => L('${n}'));`,
  M: (n) => `queueMicrotask(() => L('${n}'));`,
  P: (n) => `Promise.resolve().then(() => L('${n}'));`,
  A: (n) => `(async () => { await null; L('${n}'); })();`,
};
const names = Object.keys(ITEMS);
for (const a of names) for (const b of names) for (const c of names) {
  add([ITEMS[a]("a"), ITEMS[b]("b"), ITEMS[c]("c")].join(" "));
}

// ---------------------------------------------------------------- ordem: aninhado (o item externo agenda o interno)
const OUTER = {
  T0: (body) => `setTimeout(() => { L('out'); ${body} }, 0);`,
  T10: (body) => `setTimeout(() => { L('out'); ${body} }, 10);`,
  I: (body) => `setImmediate(() => { L('out'); ${body} });`,
  M: (body) => `queueMicrotask(() => { L('out'); ${body} });`,
  P: (body) => `Promise.resolve().then(() => { L('out'); ${body} });`,
};
const INNER = {
  T0: "setTimeout(() => L('in'), 0);",
  T10: "setTimeout(() => L('in'), 10);",
  I: "setImmediate(() => L('in'));",
  M: "queueMicrotask(() => L('in'));",
  P: "Promise.resolve().then(() => L('in'));",
};
const SIBLING = {
  none: "",
  T0: "setTimeout(() => L('sib'), 0);",
  T10: "setTimeout(() => L('sib'), 10);",
  I: "setImmediate(() => L('sib'));",
  M: "queueMicrotask(() => L('sib'));",
  P: "Promise.resolve().then(() => L('sib'));",
};
for (const o of Object.keys(OUTER)) for (const i of Object.keys(INNER)) for (const s of Object.keys(SIBLING)) {
  add(OUTER[o](INNER[i] + " L('after-in');") + " " + SIBLING[s]);
}

// ---------------------------------------------------------------- microtasks entre callbacks
for (const kind of ["setTimeout", "setImmediate"]) {
  const arg = kind === "setTimeout" ? ", 0" : "";
  add(`${kind}(() => { L('a'); Promise.resolve().then(() => L('a-p')); }${arg}); ${kind}(() => { L('b'); queueMicrotask(() => L('b-q')); }${arg}); ${kind}(() => L('c')${arg});`);
  add(`${kind}(() => { L('a'); (async () => { await null; L('a-aw'); })(); }${arg}); ${kind}(() => L('b')${arg});`);
  add(`${kind}(() => { L('a'); Promise.resolve().then(() => { L('a-p1'); return Promise.resolve(); }).then(() => L('a-p2')); }${arg}); ${kind}(() => L('b')${arg}); ${kind}(() => L('c')${arg});`);
  add(`${kind}(() => { L('a'); throw new Error('boom'); }${arg}); ${kind}(() => L('b')${arg}); Promise.resolve().then(() => L('p'));`);
  add(`${kind}(() => { L('a'); Promise.reject(new Error('r')).catch(() => L('a-catch')); }${arg}); ${kind}(() => L('b')${arg});`);
}

// ---------------------------------------------------------------- await com timer
add("(async () => { L('s'); await new Promise(r => setTimeout(r, 10)); L('after10'); await new Promise(r => setTimeout(r, 10)); L('after20'); })(); setTimeout(() => L('t15'), 15);");
add("(async () => { for (var i = 0; i < 3; i++) { await new Promise(r => setImmediate(r)); L('imm' + i); } })(); setTimeout(() => L('t0'), 0);");
add("var p = new Promise(r => setTimeout(() => r('v'), 5)); p.then(v => L('then:' + v)); setTimeout(() => L('t10'), 10);");
add("Promise.all([1, 2, 3].map(n => new Promise(r => setTimeout(() => r(n), 20 - n * 5)))).then(v => L('all:' + v.join())); Promise.race([new Promise(r => setTimeout(() => r('slow'), 15)), new Promise(r => setTimeout(() => r('fast'), 5))]).then(v => L('race:' + v));");
add("var sleep = (ms) => new Promise(r => setTimeout(r, ms)); (async () => { var order = []; await Promise.all([sleep(15).then(() => order.push('a')), sleep(5).then(() => order.push('b')), sleep(10).then(() => order.push('c'))]); L(order.join()); })();");
add("setTimeout(() => L('t5'), 5); (async () => { await 1; L('a1'); await new Promise(r => setTimeout(r, 0)); L('a2'); })();");

// ---------------------------------------------------------------- ordem por atraso, na criação inversa
for (const list of [[10, 5, 15], [15, 10, 5], [5, 5, 5], [0, 0, 0], [20, 10, 0], [0, 10, 20], [10, 10, 5], [1, 1, 10], [5, 15, 10, 20, 0], [20, 15, 10, 5, 0]]) {
  add(list.map((d, i) => `setTimeout(() => L('t${i}:${d}'), ${d});`).join(" "));
  add(list.map((d, i) => `setTimeout(() => L('t${i}:${d}'), ${d});`).join(" ") + " setImmediate(() => L('imm')); queueMicrotask(() => L('micro'));");
}
add("setTimeout(() => L('a'), 5); setTimeout(() => L('b'), 5.9); setTimeout(() => L('c'), 5.1);");
add("setTimeout(() => L('a'), '10'); setTimeout(() => L('b'), 5); setTimeout(() => L('c'), undefined); setTimeout(() => L('d'), -5); setTimeout(() => L('e'), NaN);");
add("setTimeout(() => L('big'), 2 ** 31); setTimeout(() => L('t10'), 10); setTimeout(() => L('inf'), Infinity); setTimeout(() => L('t0'), 0);");

// ---------------------------------------------------------------- setInterval
add("var n = 0; var iv = setInterval(() => { L('iv' + n); if (++n === 3) clearInterval(iv); }, 7);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n); if (++n === 3) clearInterval(iv); }, 7); setTimeout(() => L('t10'), 10); setTimeout(() => L('t20'), 20); setTimeout(() => L('t3'), 3);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n); if (++n === 3) clearInterval(iv); }, 7); setImmediate(() => L('imm')); queueMicrotask(() => L('micro'));");
add("var n = 0; var a = setInterval(() => { L('a' + n); if (++n === 3) clearInterval(a); }, 6); var m = 0; var b = setInterval(() => { L('b' + m); if (++m === 2) clearInterval(b); }, 10);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n); if (++n === 2) clearTimeout(iv); }, 8);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n); n++; Promise.resolve().then(() => L('p' + (n - 1))); if (n === 3) clearInterval(iv); }, 6);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n); if (++n === 2) throw new Error('x'); if (n === 4) clearInterval(iv); }, 6); setTimeout(() => L('t15'), 15);");
add("var n = 0; var iv = setInterval(function () { L(this === iv); if (++n === 2) clearInterval(this); }, 6);");
add("var n = 0; var iv = setInterval((a, b) => { L('args:' + a + b); if (++n === 2) clearInterval(iv); }, 6, 'x', 'y');");
add("var iv = setInterval(() => { L('never'); }, 6); clearInterval(iv); setTimeout(() => L('t10'), 10);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n); if (++n === 2) { clearInterval(iv); iv = setInterval(() => { L('second'); clearInterval(iv); }, 3); } }, 6);");
add("var n = 0; var iv = setInterval(() => { L('iv'); if (++n === 2) clearInterval(iv); }, 0);");
add("var n = 0; var iv = setInterval(() => { L('iv'); setTimeout(() => L('inner'), 0); if (++n === 2) clearInterval(iv); }, 6);");
add("var n = 0; var iv = setInterval(() => { L('iv' + n + ':' + iv._repeat + ':' + iv._idleTimeout); if (++n === 2) clearInterval(iv); }, 6);");

// ---------------------------------------------------------------- setImmediate
add("setImmediate(() => L('i1')); setImmediate(() => L('i2')); setImmediate(() => L('i3'));");
add("setImmediate(() => { L('i1'); setImmediate(() => L('i1-in')); }); setImmediate(() => L('i2'));");
add("setImmediate(() => { L('i1'); setImmediate(() => { L('i1-in'); setImmediate(() => L('i1-in-in')); }); }); setImmediate(() => L('i2')); setImmediate(() => L('i3'));");
add("setImmediate(() => { L('i1'); setTimeout(() => L('t0'), 0); setImmediate(() => L('i-in')); });");
add("setImmediate((a, b, c) => L('args:' + a + b + c + arguments.length), 1, 2);");
add("setImmediate(function (a, b) { L('args:' + a + b + ':' + arguments.length + ':' + (this === globalThis) + ':' + typeof this + ':' + this.constructor.name); }, 'x', 'y');");
add("setImmediate((...r) => L('rest:' + r.length), 1, 2, 3, 4, 5);");
add("setImmediate(() => L('ignored'), 100, 'extra');");
add("var im = setImmediate(function () { L(this === im); L(this._destroyed); }); L(im._destroyed);");
add("var im = setImmediate(() => L('x')); setImmediate(() => L(im._destroyed));");
add("setImmediate(() => { L('a'); }); queueMicrotask(() => L('m')); Promise.resolve().then(() => L('p')); setTimeout(() => L('t'), 0);");
add("var n = 0; (function loop() { L('loop' + n); if (++n < 4) setImmediate(loop); })();");
add("var n = 0; (function loop() { L('loop' + n); if (++n < 3) setImmediate(loop); })(); setTimeout(() => L('t0'), 0); setTimeout(() => L('t10'), 10);");
add("var i1 = setImmediate(() => L('i1')); var i2 = setImmediate(() => { L('i2'); clearImmediate(i3); }); var i3 = setImmediate(() => L('i3'));");
add("var i1 = setImmediate(() => { L('i1'); clearImmediate(i1); });");

// ---------------------------------------------------------------- clear: matriz criador x limpador x forma do argumento
const CREATORS = {
  setTimeout: "setTimeout(() => L('fired'), 5)",
  setInterval: "setInterval(() => { L('fired'); clearInterval(t); }, 5)",
  setImmediate: "setImmediate(() => L('fired'))",
};
const FORMS = { object: "t", number: "+t", string: "String(+t)", "number-plus-1": "(+t) + 1", "string-padded": "' ' + (+t) + ' '" };
for (const [creator, make] of Object.entries(CREATORS)) {
  for (const clearer of ["clearTimeout", "clearInterval", "clearImmediate"]) {
    for (const [form, expr] of Object.entries(FORMS)) {
      add(`var t = ${make}; ${clearer}(${expr}); L('end');`);
    }
  }
}
add("var t = setTimeout(() => L('a'), 5); clearTimeout(t); clearTimeout(t); setTimeout(() => L('b'), 10);");
add("var t = setTimeout(() => L('a'), 5); setTimeout(() => { L('b'); clearTimeout(t); }, 3); setTimeout(() => { L('c'); clearTimeout(t); }, 8);");
add("var a = setTimeout(() => { L('a'); clearTimeout(b); }, 5); var b = setTimeout(() => L('b'), 5); var c = setTimeout(() => L('c'), 5);");
add("var a = setTimeout(() => { L('a'); clearTimeout(a); }, 5); setTimeout(() => L('b'), 5);");
add("var a = setTimeout(() => L('a'), 5); var b = setTimeout(() => { L('b'); a.refresh(); }, 3); setTimeout(() => L('c'), 6);");
add("var a = setTimeout(() => L('a'), 5); setTimeout(() => { L('b'); a.refresh(); }, 8); setTimeout(() => L('c'), 10); setTimeout(() => L('d'), 14);");
add("var a = setTimeout(() => L('a'), 5); a.refresh(); a.refresh(); setTimeout(() => L('b'), 3);");
add("var a = setTimeout(() => L('a'), 2); setTimeout(() => { L('b'); a.refresh(); }, 6); setTimeout(() => L('c'), 12);");
add("var n = 0; var a = setTimeout(() => { L('a' + n); if (++n < 3) a.refresh(); }, 4);");
add("var a = setTimeout(() => L('a'), 5); a.close(); setTimeout(() => L('b'), 8);");
add("var a = setTimeout(() => L('a'), 5); a[Symbol.dispose](); var i = setImmediate(() => L('i')); i[Symbol.dispose]();");
add("var a = setTimeout(() => L('a'), 5); clearInterval(a); var b = setInterval(() => { L('b'); clearTimeout(b); }, 5);");
add("var a = setTimeout(() => L('a'), 5); var n = +a; clearTimeout(n); var b = setTimeout(() => L('b'), 5); clearTimeout(String(+b));");

// ---------------------------------------------------------------- callback: this, args, retorno
add("var t = setTimeout(function () { L(this === t); L(typeof this); L(this === globalThis); L(arguments.length); }, 1);");
add("var t = setTimeout(() => { L(this === undefined); L(typeof this); }, 1);");
for (const n of [0, 1, 2, 3, 6]) {
  const extra = Array.from({ length: n }, (_, i) => i + 10).join(", ");
  add(`setTimeout((...a) => L('timeout:' + a.length + ':' + a.join()), 1${n ? ", " + extra : ""});`);
  add(`setTimeout(function () { L('arguments:' + arguments.length); }, 1${n ? ", " + extra : ""});`);
  add(`var k = 0; var iv = setInterval((...a) => { L('interval:' + a.length + ':' + a.join()); if (++k === 2) clearInterval(iv); }, 3${n ? ", " + extra : ""});`);
  add(`setImmediate((...a) => L('immediate:' + a.length + ':' + a.join())${n ? ", " + extra : ""});`);
}
add("setTimeout((a, b) => L(typeof a + typeof b + (a === obj) + (b === undefined)), 1, globalThis.obj = {});");
add("var r = setTimeout(() => 'ret', 1); L(typeof r); setTimeout(async () => { L('async'); await null; L('async2'); }, 1); setTimeout(() => L('next'), 1);");
add("setTimeout(function named() { L(named.name); L(this.constructor.name); }, 1);");
add("class C { m() { return 'm'; } } var c = new C(); setTimeout(c.m.bind(c), 1); setTimeout(function () { L(typeof this); }.bind(5), 1);");
add("setTimeout(new Proxy(function () { L('proxy'); }, {}), 1);");
add("setTimeout(async function* () {}, 1); setTimeout(function* () { L('gen-not-run'); }, 1); L('ok');");

// ---------------------------------------------------------------- ref, unref e hasRef no laço
add("var t = setTimeout(() => L('unref-fired'), 10); t.unref(); L(t.hasRef());");
add("var t = setTimeout(() => L('a'), 10); t.unref(); setTimeout(() => L('b'), 20);");
add("var t = setTimeout(() => L('a'), 20); t.unref(); setTimeout(() => L('b'), 10);");
add("var t = setInterval(() => L('iv'), 6); t.unref(); setTimeout(() => { L('stop'); clearInterval(t); }, 15);");
add("var t = setTimeout(() => L('a'), 10); t.unref(); t.ref(); L(t.hasRef());");
add("var t = setImmediate(() => L('imm')); t.unref(); setTimeout(() => L('t'), 5);");
add("var t = setTimeout(() => L('a'), 5); clearTimeout(t); L(t.hasRef()); L(t._destroyed);");
add("var t = setTimeout(() => { L(t._destroyed + ':' + t.hasRef()); }, 5); setTimeout(() => L(t._destroyed + ':' + t.hasRef()), 10);");
add("var t = setImmediate(() => { L(t._destroyed + ':' + t.hasRef()); }); setTimeout(() => L(t._destroyed + ':' + t.hasRef()), 5);");
add("var t = setTimeout(() => { L('x'); }, 5); setTimeout(() => L(t._onTimeout === undefined ? 'undef' : typeof t._onTimeout), 10);");
add("var t = setTimeout(() => { L('orig'); }, 5); t._onTimeout = () => L('replaced');");
add("var t = setTimeout(() => { L('a'); }, 5); t._idleTimeout = 12; t.refresh(); setTimeout(() => L('b'), 8); setTimeout(() => L('c'), 14);");
add("var t = setTimeout(() => { L('orig'); }, 5); t._onTimeout = null; setTimeout(() => L('after'), 10);");

// ---------------------------------------------------------------- erro no callback
add("setTimeout(() => { throw new Error('a'); }, 1); setTimeout(() => L('b'), 1); setTimeout(() => L('c'), 5);");
add("setTimeout(() => { throw 5; }, 1); setImmediate(() => L('i')); setTimeout(() => L('c'), 5);");
add("setImmediate(() => { throw new Error('a'); }); setImmediate(() => L('b')); queueMicrotask(() => L('m'));");
add("setTimeout(() => { Promise.reject(new Error('unhandled')); L('a'); }, 1); setTimeout(() => L('b'), 5);");
add("setTimeout(() => { try { null.x; } catch (e) { L(e.name); } }, 1);");

// ---------------------------------------------------------------- recursão e reentrada
add("var n = 0; (function again() { L('t' + n); if (++n < 4) setTimeout(again, 3); })();");
add("var n = 0; (function again() { L('t' + n); if (++n < 4) setTimeout(again, 0); })(); setTimeout(() => L('x'), 10);");
add("var n = 0; (function again() { L('t' + n); if (++n < 3) { setTimeout(again, 4); setImmediate(() => L('i' + n)); } })();");
add("setTimeout(() => { L('a'); setTimeout(() => L('a-in'), 3); }, 5); setTimeout(() => { L('b'); setTimeout(() => L('b-in'), 3); }, 5); setTimeout(() => L('c'), 9);");
add("setTimeout(() => { L('a'); setTimeout(() => L('a-in'), 6); }, 5); setTimeout(() => L('b'), 10); setTimeout(() => L('c'), 12);");
add("setTimeout(() => { L('a'); setImmediate(() => L('a-imm')); queueMicrotask(() => L('a-m')); }, 5); setTimeout(() => L('b'), 5);");
add("var order = []; for (var i = 0; i < 5; i++) { (function (k) { setTimeout(() => L('t' + k), (k * 7) % 5 * 3); })(i); }");
add("for (var i = 0; i < 4; i++) setImmediate(() => L('i' + i)); for (let j = 0; j < 4; j++) setImmediate(() => L('j' + j));");
add("for (let i = 0; i < 4; i++) setTimeout(() => L('t' + i), 5 * (4 - i));");
add("var t = setTimeout(() => L('late'), 25); setTimeout(() => { L('mid'); }, 12); setImmediate(() => L('imm'));");

// ---------------------------------------------------------------- fases do laço: immediate e timeout agendados juntos
// Medido no bun (12 execuções cada): o immediate agendado dentro de immediate/timeout/topo roda antes do setTimeout(0..2)
// agendado no mesmo ponto; cadeia de 100 voltas deixa um setTimeout(1) vencer mas não um de 5 ms.
for (const d of [0, 1, 2]) {
  add(`setImmediate(() => { setImmediate(() => L('A')); setTimeout(() => L('B'), ${d}); });`);
  add(`setImmediate(() => { setTimeout(() => L('B'), ${d}); setImmediate(() => L('A')); });`);
  add(`setTimeout(() => { setImmediate(() => L('A')); setTimeout(() => L('B'), ${d}); }, 0);`);
  add(`setTimeout(() => { setTimeout(() => L('B'), ${d}); setImmediate(() => L('A')); }, 0);`);
  add(`setTimeout(() => L('B'), ${d}); setImmediate(() => L('A'));`);
}
add("setImmediate(() => { L('A1'); setImmediate(() => L('A2')); setTimeout(() => L('B'), 0); });");
add("setTimeout(() => { setImmediate(() => { L('A1'); setImmediate(() => L('A2')); setTimeout(() => L('B'), 0); }); }, 0);");
// A cadeia de 100 voltas (setTimeout(1/0) vence, setTimeout(5) não) depende do relógio de tempo real do laço do bun:
// medida em script próprio dá "B;chain-end" para 0/1 e "chain-end;B" para 5, mas dentro deste gerador (avaliado
// dentro de um callback de timer) sai "chain-end;B" para todos. Por isso fica fora do golden.
add("var cnt = 0; (function f() { L('t' + cnt); if (++cnt < 4) setTimeout(f, 0); })(); setImmediate(() => L('A'));");

// ---------------------------------------------------------------- erros de argumento (code) e avisos de atraso
for (const call of ["setTimeout()", "setTimeout(1)", "setInterval()", "setInterval('x')", "setImmediate()", "setImmediate({})", "queueMicrotask()", "queueMicrotask(1)", "queueMicrotask('x')", "queueMicrotask({})", "queueMicrotask(null)"]) {
  add(`try { ${call}; L('no-throw'); } catch (e) { L(e.name + '|' + e.code + '|' + e.message + '|' + (e instanceof TypeError) + '|' + Object.hasOwn(e, 'code')); }`);
}
add("var C = Object.getPrototypeOf(setTimeout(() => {}, 1)).constructor; try { C(); } catch (e) { L(e.name + '|' + e.code + '|' + e.message); }");
add("var C = Object.getPrototypeOf(setImmediate(() => {})).constructor; try { C(); } catch (e) { L(e.name + '|' + e.code + '|' + e.message); }");
// O aviso chega pelo evento `warning` (um tick depois); delay que não é número de fato não avisa.
for (const delay of ["-5", "NaN", "Infinity", "-Infinity", "2 ** 31", "2147483647", "2147483648.5", "-0.5", "-0", "0", "0.5", "'abc'", "'-5'", "null", "true", "{}", "1e21"]) {
  add(`process.on('warning', (w) => L(w.name + '|' + w.code + '|' + JSON.stringify(w.message))); setTimeout(() => L('fired'), ${delay}).unref(); setTimeout(() => L('end'), 5);`);
  add(`process.on('warning', (w) => L(w.name + '|' + JSON.stringify(w.message))); var i = setInterval(() => { L('iv'); clearInterval(i); }, ${delay}); setTimeout(() => L('end'), 5);`);
}
add("process.on('warning', (w) => L(w.name)); setImmediate(() => L('imm'), -1); setTimeout(() => L('end'), 5);");
// Ordem do setImmediate, do setTimeout 0, de microtask, de promise e de nextTick no topo do programa.
add("setTimeout(() => L('t0'), 0); setImmediate(() => L('imm')); queueMicrotask(() => L('mt')); Promise.resolve().then(() => L('p')); process.nextTick(() => L('tick'));");
add("var t = setTimeout(() => {}, 1); L(typeof t[Symbol.toPrimitive]() + '|' + (+t === t[Symbol.toPrimitive]()) + '|' + (String(t) === String(+t)) + '|' + (t.close() === t) + '|' + (t.refresh() === t));");
add("var t = setTimeout(() => L('NO'), 1); clearTimeout(String(+t)); var u = setTimeout(() => L('NO2'), 1); clearTimeout(+u);");

const quote = (s) => JSON.stringify(s);
async function round() {
  const rows = [];
  for (const body of programs) {
    const program = SENTINEL + body;
    globalThis.R = "";
    const saved = {};
    for (const n of FUNCTIONS.concat(["queueMicrotask"])) saved[n] = globalThis[n];
    (0, eval)(prelude);
    try {
      (0, eval)(program);
    } catch (e) {
      globalThis.R += "sync-throw:" + e.name + ";";
    }
    await new Promise((resolve) => saved.setTimeout(resolve, 50));
    rows.push([program, String(globalThis.R)]);
    for (const n of Object.keys(saved)) globalThis[n] = saved[n];
  }
  return rows;
}

(async () => {
  process.on("uncaughtException", () => {});
  process.on("unhandledRejection", () => {});
  const rounds = [];
  for (let i = 0; i < REPEAT; i++) rounds.push(await round());
  const kept = [];
  let dropped = 0;
  rounds[0].forEach(([program, result], index) => {
    if (rounds.every((rows) => rows[index][1] === result)) kept.push([program, result]);
    else {
      dropped++;
      console.error("instável: " + program + " => " + rounds.map((rows) => rows[index][1]).join(" | "));
    }
  });
  console.error("casos mantidos: " + kept.length + ", descartados: " + dropped);
  process.stdout.write(require("./golden-prelude.js").assertPublicResult(kept.map(([program, result]) => quote(program) + "\t" + quote(result) + "\t0").join("\n") + "\n"));
  process.exit(0);
})();
