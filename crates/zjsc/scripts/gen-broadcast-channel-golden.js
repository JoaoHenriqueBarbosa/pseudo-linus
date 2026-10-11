// Gera tests/golden/broadcast_channel_bun.tsv: `BroadcastChannel` do global medido no bun 1.4.2 (descritor do global,
// `length`, `name`, chaves do construtor e do protótipo, descritores dos acessores e métodos, erros de chamada sem
// `new`, sem argumento, com `this` errado, `postMessage` de função e depois de `close`, conversão do nome, inspect).
// Entrega entre canais do mesmo nome (assíncrona, nunca ao remetente, nunca a outro nome) fica em programas à parte:
// ver `scripts/gen-broadcast-channel-delivery-golden.js` quando existir.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-broadcast-channel-golden.js > tests/golden/broadcast_channel_bun.tsv
const { emitRow } = require("./golden-prelude.js");

const HELPER =
  "var S = function (v) { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return 'symbol'; " +
  "if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v); " +
  "if (v !== null && typeof v === 'object') { try { return JSON.stringify(v) } catch (e) { return 'object' } } return String(v) };\n" +
  "var E = function (e) { return e.name + '|' + e.message + '|' + e.code + '|' + (e instanceof Error) };\n";
const programs = [];
const expr = (code) => programs.push(HELPER + `try { R = S(${code}) } catch (e) { R = E(e) }`);

const N = "BroadcastChannel";
const P = `${N}.prototype`;
expr(`(function(d){ return [typeof d.value, d.writable, d.enumerable, d.configurable, 'get' in d] })(Object.getOwnPropertyDescriptor(globalThis, '${N}'))`);
expr(`${N}.length`);
expr(`${N}.name`);
expr(`Object.getOwnPropertyNames(${N})`);
expr(`Object.getOwnPropertyNames(${P})`);
expr(`Object.getPrototypeOf(${N}) === EventTarget`);
expr(`Object.getPrototypeOf(${P}) === EventTarget.prototype`);
expr(`${P}.constructor === ${N}`);
expr(`${P}[Symbol.toStringTag]`);
expr(`Object.prototype.toString.call(new ${N}('a'))`);
for (const m of ["postMessage", "close", "ref", "unref"]) {
  expr(`(function(d){ return [d.enumerable, d.writable, d.configurable, d.value.length, d.value.name] })(Object.getOwnPropertyDescriptor(${P}, '${m}'))`);
}
for (const p of ["name", "onmessage", "onmessageerror"]) {
  expr(`(function(d){ return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.get.length, d.set && d.set.name, d.set && d.set.length] })(Object.getOwnPropertyDescriptor(${P}, '${p}'))`);
}
expr(`${N}('a')`);
expr(`new ${N}()`);
expr(`new ${N}(undefined).name`);
expr(`new ${N}(null).name`);
expr(`new ${N}(12).name`);
expr(`new ${N}({ toString() { return 'x' } }).name`);
expr(`new ${N}(Symbol())`);
expr(`new ${N}('a').onmessage`);
expr(`new ${N}('a').onmessageerror`);
expr(`new ${N}('a').postMessage()`);
expr(`new ${N}('a').postMessage(() => 1)`);
expr(`${P}.postMessage.call({}, 1)`);
expr(`${P}.close.call({})`);
expr(`Object.getOwnPropertyDescriptor(${P}, 'name').get.call({})`);
expr(`(function(c){ c.close(); return c.postMessage(1) })(new ${N}('a'))`);
expr(`(function(c){ c.close(); return c.close() })(new ${N}('a'))`);
expr(`(function(c){ c.onmessage = 5; return c.onmessage })(new ${N}('a'))`);
expr(`(function(c){ var f = function () {}; c.onmessage = f; return c.onmessage === f })(new ${N}('a'))`);
expr(`(function(c){ c.onmessage = function () {}; c.onmessage = null; return c.onmessage })(new ${N}('a'))`);
expr(`typeof new ${N}('a').ref()`);

for (const source of programs) {
  const sourceAscii = source.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  (0, eval)("var R");
  (0, eval)(sourceAscii);
  emitRow(JSON.stringify(sourceAscii) + "\t" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));
}
// Cada canal aberto mantém o laço de eventos vivo no bun (por isso existe `unref`); o gerador encerra à mão.
process.exit(0);
