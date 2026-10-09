// Gera tests/golden/wasm_api_bun.tsv: cerca de 800 programas da superfície de API JS do WebAssembly (nomes e
// descritores de propriedades, Module, Instance, Memory, Table, Global, Tag, Exception, validate, compile,
// instantiate, importObject, conversões de argumentos e resultados, traps), avaliados no bun. Complementa
// scripts/gen-wasm-js-golden.js (que cobre o executor: instruções, traps e GC); aqui o foco é a API.
// Cada programa registra eventos no array global `log` e usa os auxiliares (L, E, D, P, T, C, R, N, F, mk, sec,
// str, mut e os módulos binários) de tests/golden/wasm_api_bun_harness.js, o mesmo texto que
// tests/wasm_api_bun_golden.rs embute. Colunas: fonte, depois o JSON do log depois de esvaziar as microtarefas,
// ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona. Cada programa roda num processo
// bun próprio, com timeout. Uso:
//   bun scripts/gen-wasm-api-golden.js > tests/golden/wasm_api_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_api_bun_harness.js"), "utf8");
(0, eval)(harness);
const G = globalThis;

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
const bytes = (name) => "new Uint8Array([" + Array.from(G[name]).join(",") + "])";

// Nomes e descritores.
const protos = ["Module", "Instance", "Memory", "Table", "Global", "Tag", "Exception", "CompileError", "LinkError", "RuntimeError"];
add("L(N(WebAssembly))", "L(Object.keys(WebAssembly).length)", "L(String(WebAssembly))", "L(typeof WebAssembly)", "L(Object.getPrototypeOf(WebAssembly) === Object.prototype)",
  "L(WebAssembly[Symbol.toStringTag])", "L(F(WebAssembly, Symbol.toStringTag))");
for (const name of ["validate", "compile", "instantiate", "compileStreaming", "instantiateStreaming", "Module", "Instance", "Memory", "Table", "Global", "Tag", "Exception", "CompileError", "LinkError", "RuntimeError", "JSTag", "Function", "Suspending", "promising"]) {
  add(`L(F(WebAssembly, '${name}'))`, `L(typeof WebAssembly.${name})`);
}
for (const name of ["validate", "compile", "instantiate"]) {
  add(`L(WebAssembly.${name}.length); L(WebAssembly.${name}.name); L(String(WebAssembly.${name}))`);
}
for (const name of protos) {
  const C = `WebAssembly.${name}`;
  add(
    `L(N(${C}.prototype))`,
    `L(N(${C}))`,
    `L(${C}.length); L(${C}.name); L(typeof ${C})`,
    `L(F(${C}, 'prototype')); L(F(${C}.prototype, 'constructor')); L(${C}.prototype.constructor === ${C})`,
    `L(${C}.prototype[Symbol.toStringTag]); L(F(${C}.prototype, Symbol.toStringTag))`,
    `L(Object.getPrototypeOf(${C}.prototype) === ${/Error$/.test(name) ? "Error.prototype" : "Object.prototype"})`,
    `L(Object.getPrototypeOf(${C}) === ${/Error$/.test(name) ? "Error" : "Function.prototype"})`,
    `L(Object.prototype.toString.call(${C}.prototype))`,
    `L(Object.isExtensible(${C}.prototype)); L(Object.isFrozen(${C}))`
  );
}
for (const [cls, props] of [
  ["Module", ["exports", "imports", "customSections"]],
  ["Memory", []],
  ["Table", []],
  ["Global", []],
]) {
  for (const p of props) add(`L(F(WebAssembly.Module, '${p}')); L(WebAssembly.Module.${p}.length); L(WebAssembly.Module.${p}.name)`);
}
for (const [cls, props] of [
  ["Memory", ["grow", "buffer", "toString", "type"]],
  ["Table", ["length", "grow", "set", "get", "type"]],
  ["Global", ["value", "valueOf", "type"]],
  ["Tag", ["type"]],
  ["Exception", ["is", "getArg", "stack"]],
  ["Instance", ["exports"]],
]) {
  for (const p of props) add(`var d = Object.getOwnPropertyDescriptor(WebAssembly.${cls}.prototype, '${p}'); L(F(WebAssembly.${cls}.prototype, '${p}')); L(d ? (typeof d.value) + typeof d.get + typeof d.set : 'none'); L(d && d.value ? d.value.length + ':' + d.value.name : 'x')`);
}
for (const p of ["buffer", "value", "length", "exports"]) {
  const cls = { buffer: "Memory", value: "Global", length: "Table", exports: "Instance" }[p];
  add(`L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.${cls}.prototype, '${p}').get.call({})))`, `L(T(() => WebAssembly.${cls}.prototype.${p}))`);
}

// Entradas que não são bytes.
const nonBytes = ["1", "'x'", "null", "undefined", "({})", "[]", "[0, 97, 115, 109, 1, 0, 0, 0]", "new ArrayBuffer(0)", "new ArrayBuffer(4)", "new Uint8Array(0)", "new Uint16Array(4)", "new Int8Array(8)", "new Uint8ClampedArray(8)", "new Float32Array(2)", "new DataView(new ArrayBuffer(8))", "new SharedArrayBuffer(8)", "true", "Symbol('s')", "1n", "function () {}", "new Uint32Array(new ArrayBuffer(8))", "new BigInt64Array(1)"];
for (const v of nonBytes) {
  add(
    `L(T(() => new WebAssembly.Module(${v})))`,
    `L(T(() => WebAssembly.validate(${v})))`,
    `P(WebAssembly.compile(${v}), 'c')`,
    `P(WebAssembly.instantiate(${v}), 'i')`
  );
}
add(
  "L(T(() => new WebAssembly.Module()))", "L(T(() => WebAssembly.validate()))", "P(WebAssembly.compile(), 'c')", "P(WebAssembly.instantiate(), 'i')",
  `var u = ${bytes("ADD")}; L(T(() => new WebAssembly.Module(u.buffer))); L(T(() => new WebAssembly.Module(new DataView(u.buffer))))`,
  `var u = ${bytes("ADD")}; L(WebAssembly.validate(u.buffer)); L(WebAssembly.validate(u.subarray(0))); L(T(() => WebAssembly.validate(new Uint16Array(u.buffer.slice(0, 8)))))`,
  `var s = ${bytes("ADD")}; var w = new Uint8Array(s.length + 4); w.set(s, 4); L(T(() => new WebAssembly.Module(w))); L(T(() => new WebAssembly.Module(w.subarray(4))))`,
  `var b = new ArrayBuffer(${G.ADD.length}); new Uint8Array(b).set(${bytes("ADD")}); var m = new WebAssembly.Module(b); new Uint8Array(b)[0] = 9; L(WebAssembly.Module.exports(m).length)`,
  `var s = ${bytes("ADD")}; var m = new WebAssembly.Module(s); s[0] = 9; L(JSON.stringify(WebAssembly.Module.exports(m)))`
);

// Erros de compilação: cada prefixo truncado e cada byte alterado.
for (let n = 0; n < G.ADD.length; n++) add(`L(T(() => new WebAssembly.Module(${bytes("ADD")}.subarray(0, ${n}))))`);
for (let n = 0; n < G.ADD.length; n += 2) {
  for (const v of [0xff, 0x00]) add(`L(T(() => new WebAssembly.Module(mut(${bytes("ADD")}, ${n}, ${v}))))`);
}
for (let n = 0; n < G.IMP.length; n += 3) add(`L(T(() => new WebAssembly.Module(mut(${bytes("IMP")}, ${n}, 0x7f))))`);
add(
  "L(T(() => new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 2, 0, 0, 0]))))",
  "L(T(() => new WebAssembly.Module(new Uint8Array([0, 97, 115, 110, 1, 0, 0, 0]))))",
  "L(T(() => new WebAssembly.Module(new Uint8Array([0, 97, 115, 109]))))",
  "L(T(() => new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0]))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [0]), sec(1, [0])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(3, [0]), sec(1, [0])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(99, [0])))))",
  "L(T(() => new WebAssembly.Module(mk([1, 5, 0]))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 5])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x61, 0, 0])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [1, 0, 1]), sec(5, [1, 0, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [2, 0, 1, 0, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [1, 0, 0x81, 0x80, 0x04])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [1, 1, 2, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [1, 2, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(4, [1, 0x70, 1, 2, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(4, [1, 0x7f, 0, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(6, [1, 0x7f, 0, 0x42, 1, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(6, [1, 0x7f, 0, 0x41, 1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(7, [1].concat(str('a'), [0, 0]))))))",
  "L(T(() => new WebAssembly.Module(mk(sec(7, [2].concat(str('a'), [3, 0], str('a'), [3, 0]))))))",
  "L(T(() => new WebAssembly.Module(mk(sec(7, [1, 2, 0xc3, 0x28, 0, 0])))))",
  "L(T(() => new WebAssembly.Module(mk(typeVoidX(), sec(3, [1, 0]), sec(10, [1, 2, 0, 0x0b], 1)))))".replace("typeVoidX()", "sec(1, [1, 0x60, 0, 0])"),
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(10, [1, 2, 0, 0x0c])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 1, 0x7f]), sec(3, [1, 0]), sec(10, [1, 2, 0, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(10, [1, 4, 0, 0x41, 1, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(10, [1, 3, 0, 0xff, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(10, [1, 4, 0, 0x20, 0, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(10, [1, 4, 0, 0x10, 5, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(8, [1]), sec(10, [1, 2, 0, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(8, [3]), sec(10, [1, 2, 0, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(2, [1].concat(str('m'), str('f'), [0, 3]))))))",
  "L(T(() => new WebAssembly.Module(mk(sec(2, [1].concat(str('m'), str('f'), [9, 0]))))))",
  "L(T(() => new WebAssembly.Module(mk(sec(0, [5, 97])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(0, [])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(12, [1])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [2, 0x60, 0, 0, 0x60, 0, 0]), sec(3, [1, 1]), sec(10, [1, 2, 0, 0x0b])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(10, [1, 2, 0, 0x0b]), sec(11, [1, 0, 0x41, 0, 0x0b, 0])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [1, 0, 1]), sec(11, [1, 0, 0x41, 0, 0x0b, 1, 7])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(5, [1, 0, 1]), sec(11, [1, 0, 0x41, 0x80, 0x80, 0x04, 0x0b, 1, 7])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(9, [1, 0, 0x41, 0, 0x0b, 0])))))",
  "L(T(() => new WebAssembly.Module(mk(sec(12, [1]), sec(11, [1, 1, 1, 7])))))"
);

// validate.
for (const name of ["EMPTY", "ADD", "IMP", "MEM", "TAB", "GLB", "START", "TRAP", "CUSTOM", "ALLIMP", "ALLEXP", "TAGIMP", "TAGEXP", "UEXP"]) {
  add(`L(WebAssembly.validate(${name}))`);
}
add("L(WebAssembly.validate.call(null, ADD))", "L(WebAssembly.validate(ADD, 1, 2))", "L(T(() => new WebAssembly.validate(ADD)))", "L(WebAssembly.validate(ADD.slice(0, 4)))");

// Module.exports, imports, customSections.
for (const name of ["EMPTY", "ADD", "IMP", "MEM", "TAB", "GLB", "START", "TRAP", "CUSTOM", "ALLIMP", "ALLEXP", "TAGIMP", "TAGEXP", "UEXP"]) {
  add(
    `var m = new WebAssembly.Module(${name}); L(JSON.stringify(WebAssembly.Module.exports(m))); L(JSON.stringify(WebAssembly.Module.imports(m)))`,
    `var m = new WebAssembly.Module(${name}); var e = WebAssembly.Module.exports(m); L(Array.isArray(e)); L(e.length); L(e.length ? N(e[0]) : 'vazio'); L(e.length ? F(e[0], 'name') : 'x'); L(Object.isExtensible(e))`,
    `var m = new WebAssembly.Module(${name}); var e = WebAssembly.Module.imports(m); L(e.length ? N(e[0]) : 'vazio'); L(e.length ? Object.getPrototypeOf(e[0]) === Object.prototype : 'x')`,
    `var m = new WebAssembly.Module(${name}); L(WebAssembly.Module.exports(m) === WebAssembly.Module.exports(m))`
  );
}
for (const name of ["a", "bc", "", "zz", "A", "a\\u0000"]) {
  add(`var m = new WebAssembly.Module(CUSTOM); var s = WebAssembly.Module.customSections(m, '${name}'); L(s.length); L(s.map(function (b) { return b instanceof ArrayBuffer ? Array.from(new Uint8Array(b)).join('.') : 'nao' }).join('|'))`);
}
add(
  "var m = new WebAssembly.Module(CUSTOM); L(T(() => WebAssembly.Module.customSections(m)))",
  "var m = new WebAssembly.Module(CUSTOM); L(T(() => WebAssembly.Module.customSections(m, undefined)))",
  "var m = new WebAssembly.Module(CUSTOM); L(T(() => WebAssembly.Module.customSections(m, 1)))",
  "var m = new WebAssembly.Module(CUSTOM); L(T(() => WebAssembly.Module.customSections(m, { toString() { return 'a' } }).length))",
  "var m = new WebAssembly.Module(CUSTOM); L(T(() => WebAssembly.Module.customSections(m, Symbol('s'))))",
  "L(T(() => WebAssembly.Module.customSections({}, 'a')))",
  "L(T(() => WebAssembly.Module.customSections(1, 'a')))",
  "L(T(() => WebAssembly.Module.exports({})))", "L(T(() => WebAssembly.Module.exports()))", "L(T(() => WebAssembly.Module.exports(null)))",
  "L(T(() => WebAssembly.Module.imports({})))", "L(T(() => WebAssembly.Module.imports()))", "L(T(() => WebAssembly.Module.imports(ADD)))",
  "var m = new WebAssembly.Module(CUSTOM); var a = WebAssembly.Module.customSections(m, 'a'); var b = WebAssembly.Module.customSections(m, 'a'); L(a[0] === b[0]); L(a[0].byteLength); L(Object.isFrozen(a))",
  "var m = new WebAssembly.Module(ADD); L(Object.prototype.toString.call(m)); L(String(m)); L(m instanceof WebAssembly.Module); L(Object.getPrototypeOf(m) === WebAssembly.Module.prototype)",
  "var m = new WebAssembly.Module(ADD); L(Object.getOwnPropertyNames(m).length); L(Object.isExtensible(m)); L(JSON.stringify(m))",
  "class M extends WebAssembly.Module {}; var m = new M(ADD); L(m instanceof M); L(Object.getPrototypeOf(m) === M.prototype); L(WebAssembly.Module.exports(m).length)",
  "var m = Reflect.construct(WebAssembly.Module, [ADD], Object); L(Object.getPrototypeOf(m) === Object.prototype); L(T(() => WebAssembly.Module.exports(m)))",
  "L(T(() => WebAssembly.Module.prototype.constructor.call({}, ADD)))",
  "L(T(() => Reflect.apply(WebAssembly.Module, null, [ADD])))",
  "L(T(() => WebAssembly.Module.call({}, ADD)))"
);

// Instance.
add(
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(Object.keys(i.exports).join(',')); L(N(i.exports)); L(Object.isFrozen(i.exports)); L(Object.getPrototypeOf(i.exports) === null); L(Object.isExtensible(i.exports))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(Object.getOwnPropertySymbols(i.exports).length); L(Reflect.ownKeys(i.exports).join(','))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(['f','t','mem','e','g'].map(function (k) { return k + ':' + F(i.exports, k) }).join(' '))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(['f','t','mem','e','g'].map(function (k) { return Object.prototype.toString.call(i.exports[k]) }).join(' '))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(typeof i.exports.f); L(i.exports.t instanceof WebAssembly.Table); L(i.exports.mem instanceof WebAssembly.Memory); L(i.exports.e instanceof WebAssembly.Tag); L(i.exports.g instanceof WebAssembly.Global); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); 'use strict'; L(T(() => { 'use strict'; i.exports.f = 1 })); L(T(() => { 'use strict'; delete i.exports.f })); L(T(() => { 'use strict'; i.exports.n = 1 }))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(i.exports === i.exports); L(i.exports.f === i.exports.f); L(Object.getOwnPropertyDescriptor(WebAssembly.Instance.prototype, 'exports').get.call(i) === i.exports)",
  "var m = new WebAssembly.Module(ALLEXP); var a = new WebAssembly.Instance(m); var b = new WebAssembly.Instance(m); L(a.exports === b.exports); L(a.exports.f === b.exports.f); L(a.exports.mem === b.exports.mem)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(UEXP)); L(Object.keys(i.exports).join(',')); L(i.exports['\\u00e9'].name); L(WebAssembly.Module.exports(new WebAssembly.Module(UEXP))[0].name)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(EMPTY)); L(N(i.exports)); L(Object.isFrozen(i.exports)); L(Object.prototype.toString.call(i)); L(String(i)); L(N(i))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); L(Object.prototype.toString.call(i)); L(i[Symbol.toStringTag]); L(i instanceof WebAssembly.Instance); L(Object.getPrototypeOf(i) === WebAssembly.Instance.prototype)",
  "class I extends WebAssembly.Instance {}; var i = new I(new WebAssembly.Module(ADD)); L(i instanceof I); L(i.exports.add(1, 2))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD), undefined); L(i.exports.add(1, 2))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD), null); L(i.exports.add(1, 2))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ADD), 1)))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ADD), 'x')))",
  "L(T(() => new WebAssembly.Instance()))", "L(T(() => new WebAssembly.Instance(null)))", "L(T(() => new WebAssembly.Instance(ADD)))", "L(T(() => new WebAssembly.Instance({})))",
  "L(T(() => new WebAssembly.Instance(WebAssembly.Module.prototype)))",
  "L(T(() => WebAssembly.Instance.prototype.exports))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(START)); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var f = i.exports.add; L(Object.isFrozen(f)); L(Object.isExtensible(f)); L(N(f)); L(F(f, 'name')); L(F(f, 'length')); L(Object.getPrototypeOf(f) === Function.prototype); L(String(f)); L(f.hasOwnProperty('prototype'))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var f = i.exports.add; L(T(() => new f(1, 2))); L(f.call(null, 3, 4)); L(f.apply(undefined, [5, 6])); L(f.bind(null, 1)(2)); L(Reflect.apply(f, {}, [7, 8]))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var f = i.exports.add; f.x = 1; L(f.x); L(Object.keys(f).join(','))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); L(i.exports.add.constructor === Function); L(i.exports.add instanceof Function); L(Object.prototype.toString.call(i.exports.add)); L(typeof i.exports.add.toString())"
);

// Memory.
const memCases = [
  "{ initial: 0 }", "{ initial: 1 }", "{ initial: 2 }", "{ initial: 0, maximum: 0 }", "{ initial: 1, maximum: 1 }", "{ initial: 1, maximum: 3 }", "{ initial: 2, maximum: 1 }",
  "{ initial: 65536 }", "{ initial: 65537 }", "{ initial: 1, maximum: 65536 }", "{ initial: 1, maximum: 65537 }", "{ initial: 1, maximum: 4294967296 }", "{ initial: 4294967296 }",
  "{ initial: -1 }", "{ initial: 1.5 }", "{ initial: '1' }", "{ initial: 'a' }", "{ initial: NaN }", "{ initial: Infinity }", "{ initial: null }", "{ initial: undefined }", "{ initial: true }", "{ initial: {} }", "{ initial: 1n }",
  "{ minimum: 1 }", "{ minimum: 1, initial: 1 }", "{ minimum: 2, initial: 1 }", "{ initial: 1, maximum: undefined }", "{ initial: 1, maximum: null }", "{ initial: 1, maximum: -1 }", "{ initial: 1, maximum: 'x' }", "{ initial: 1, maximum: NaN }",
  "{ initial: 1, shared: true }", "{ initial: 1, maximum: 2, shared: true }", "{ initial: 1, shared: false }", "{ initial: 1, maximum: 2, shared: 1 }", "{ initial: 1, maximum: 2, shared: 0 }", "{ initial: 1, maximum: 2, shared: 'x' }", "{ initial: 1, maximum: 2, shared: {} }",
  "{ initial: 1, index: 'i64' }", "{ initial: 1, index: 'i32' }", "{ initial: 1, index: 'x' }", "{ initial: { valueOf() { return 2 } } }", "{ get initial() { throw new RangeError('g') } }",
  "undefined", "null", "1", "'x'", "[]", "{}", "function () {}", "new Proxy({ initial: 1 }, {})",
];
for (const spec of memCases) {
  add(
    `L(T(() => new WebAssembly.Memory(${spec})))`,
    `L(C(() => { var m = new WebAssembly.Memory(${spec}); return m.buffer.byteLength + ',' + (m.buffer instanceof ArrayBuffer) + ',' + (m.buffer instanceof SharedArrayBuffer) + ',' + Object.prototype.toString.call(m.buffer) }))`
  );
}
add(
  "L(T(() => WebAssembly.Memory({ initial: 1 })))",
  "L(T(() => WebAssembly.Memory.call({}, { initial: 1 })))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(m.buffer === m.buffer); L(Object.isFrozen(m.buffer)); L(m.buffer.resizable); L(m.buffer.maxByteLength); L(m.buffer.detached)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 4 }); L(m.grow(0)); L(m.grow(1)); L(m.grow(2)); L(m.buffer.byteLength); L(T(() => m.grow(1))); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1 }); var b = m.buffer; L(m.grow(0)); L(b === m.buffer); L(b.byteLength); L(b.detached)",
  "var m = new WebAssembly.Memory({ initial: 1 }); var b = m.buffer; L(m.grow(1)); L(b === m.buffer); L(b.byteLength); L(b.detached); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1 }); var b = m.buffer; var u = new Uint8Array(b); u[0] = 7; m.grow(1); L(u.length); L(u[0]); L(new Uint8Array(m.buffer)[0]); L(u.byteOffset)",
  "var m = new WebAssembly.Memory({ initial: 1 }); var b = m.buffer; new Uint8Array(b)[5] = 9; m.grow(1); L(new Uint8Array(m.buffer)[5]); L(T(() => b.slice(0)))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => structuredClone(m.buffer)))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => m.buffer.transfer())); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => m.buffer.resize(1)))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => { m.buffer = 1; return 1 })); L(T(() => { 'use strict'; m.buffer = 1 }))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); var b = m.buffer; L(b instanceof SharedArrayBuffer); L(b.byteLength); L(m.grow(1)); L(b === m.buffer); L(b.byteLength); L(m.buffer.byteLength); L(Object.isFrozen(m.buffer)); L(b.growable)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); L(m.buffer === m.buffer); L(Object.isFrozen(m.buffer)); L(Object.isSealed(m.buffer))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); var b = m.buffer; L(m.grow(1)); L(b.byteLength); L(m.buffer.byteLength); L(m.buffer === b); L(m.buffer === m.buffer); L(Object.isFrozen(m.buffer)); L(Object.isFrozen(b)); L(b.growable); L(b.maxByteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); var b = m.buffer; var f = m.toFixedLengthBuffer(); L(f === b); L(f === m.buffer); L(f instanceof SharedArrayBuffer); L(f.byteLength); L(Object.isFrozen(f)); L(T(() => m.grow(1))); L(f.byteLength); var g = m.toFixedLengthBuffer(); L(g === f); L(g.byteLength); L(g === m.buffer)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); var b = m.buffer; var r = m.toResizableBuffer(); L(r === b); L(r === m.buffer); L(r instanceof SharedArrayBuffer); L(r.byteLength); L(r.growable); L(r.maxByteLength); L(Object.isFrozen(r)); L(m.grow(1)); L(r.byteLength); L(b.byteLength); L(m.buffer === r); L(m.toResizableBuffer() === r)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); var r = m.toResizableBuffer(); var f = m.toFixedLengthBuffer(); L(f === r); L(f.growable); L(m.buffer === f); L(m.buffer === m.buffer)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); var u = new Uint8Array(m.buffer); u[3] = 5; m.grow(1); L(u.length); L(new Uint8Array(m.buffer)[3]); L(new Uint8Array(m.buffer).length); L(T(() => m.buffer.slice(0, 4).byteLength))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); L(m.grow(0)); L(m.buffer.byteLength); L(T(() => m.grow(3))); L(m.grow(2)); L(T(() => m.grow(1))); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 2, maximum: 2, shared: true }); L(T(() => m.grow(1))); L(m.grow(0)); L(m.buffer === m.buffer); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 0, maximum: 1, shared: true }); L(m.buffer.byteLength); L(m.buffer instanceof SharedArrayBuffer); L(m.grow(1)); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); L(T(() => structuredClone(m.buffer).byteLength)); L(Atomics.add(new Int32Array(m.buffer), 0, 5)); L(Atomics.load(new Int32Array(m.buffer), 0))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true }); L(T(() => m.buffer.grow(131072))); L(T(() => m.buffer.resize(131072))); L(T(() => m.buffer.transfer))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,5,4,1,3,1,2,7,7,1,3,109,101,109,2,0]))); var m = i.exports.mem; L(m instanceof WebAssembly.Memory); L(m.buffer instanceof SharedArrayBuffer); L(m.buffer.byteLength); L(m.grow(1)); L(m.buffer.byteLength); L(T(() => m.grow(1))); L(i.exports.mem === m); L(m.buffer === m.buffer)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); var i = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,2,11,1,1,109,3,109,101,109,2,3,1,2,7,7,1,3,109,101,109,2,0])), { m: { mem: m } }); L(i.exports.mem === m); L(i.exports.mem.buffer === m.buffer); L(m.grow(1)); L(i.exports.mem.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,2,11,1,1,109,3,109,101,109,2,3,1,2])), { m: { mem: m } })))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,2,9,1,1,109,3,109,101,109,2,0,1])), { m: { mem: m } })))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,2,11,1,1,109,3,109,101,109,2,3,1,3])), { m: { mem: m } })))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 1 }); L(T(() => m.grow(1))); L(m.grow(0)); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => m.grow())); L(T(() => m.grow(-1))); L(T(() => m.grow('x'))); L(T(() => m.grow(1.5))); L(T(() => m.grow(4294967296))); L(T(() => m.grow(65536))); L(T(() => m.grow(NaN))); L(T(() => m.grow({})));",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(m.grow('2')); L(m.buffer.byteLength); L(m.grow(true)); L(m.buffer.byteLength); L(m.grow(null)); L(m.grow(undefined === 1 ? 0 : '0'))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => m.grow(1n)))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => m.grow(Symbol())))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(m.grow({ valueOf() { return 1 } })); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => WebAssembly.Memory.prototype.grow.call({}, 1))); L(T(() => WebAssembly.Memory.prototype.grow.call(m, 1)))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(Object.prototype.toString.call(m)); L(String(m)); L(m[Symbol.toStringTag]); L(m.toString === Object.prototype.toString); L(N(m)); L(JSON.stringify(m))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(m instanceof WebAssembly.Memory); L(Object.getPrototypeOf(m) === WebAssembly.Memory.prototype); L(Object.isExtensible(m))",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => m.type())); L(typeof m.type)",
  "var m = new WebAssembly.Memory({ initial: 1 }); L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.Memory.prototype, 'buffer').get.call(m).byteLength))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(MEM)); var b = i.exports.mem.buffer; i.exports.mem.grow(1); L(b.byteLength); L(i.exports.mem.buffer.byteLength); L(i.exports.mem.grow(0))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: m } }); L(i.exports.mem === undefined); L(m.grow(1))",
  "var m = new WebAssembly.Memory({ initial: 2 }); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: m } }); L('ok')",
  "var m = new WebAssembly.Memory({ initial: 0 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: m } })))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 5 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: m } })))",
  "var m = new WebAssembly.Memory({ initial: 1, shared: true, maximum: 2 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: m } })))",
  "var m = new WebAssembly.Memory({ initial: 1 }); var i = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f], [0x7f], [0x20, 0, 0x28, 2, 0])), undefined); var u = new Uint8Array(i.exports.mem.buffer); u[4] = 42; L(i.exports.f(4)); L(u[4])",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f], [0x7f], [0x20, 0, 0x28, 2, 0]))); L(i.exports.f(65532)); L(T(() => i.exports.f(65533))); L(T(() => i.exports.f(-1)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f, 0x7f], [], [0x20, 0, 0x20, 1, 0x36, 2, 0]))); i.exports.f(8, 305419896); L(new Uint32Array(i.exports.mem.buffer)[2].toString(16)); L(T(() => i.exports.f(65536, 1)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f], [0x7f], [0x20, 0, 0x40, 0]))); L(i.exports.f(1)); L(i.exports.mem.buffer.byteLength); L(i.exports.f(70000)); L(i.exports.f(0))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([], [0x7f], [0x3f, 0]))); var b = i.exports.mem.buffer; L(i.exports.f()); i.exports.mem.grow(2); L(i.exports.f()); L(b.byteLength)"
);

// Table.
const tabCases = [
  "{ element: 'anyfunc', initial: 0 }", "{ element: 'anyfunc', initial: 1 }", "{ element: 'anyfunc', initial: 2, maximum: 1 }", "{ element: 'anyfunc', initial: 1, maximum: 2 }",
  "{ element: 'externref', initial: 1 }", "{ element: 'externref', initial: 2, maximum: 4 }", "{ element: 'funcref', initial: 1 }", "{ element: 'i32', initial: 1 }", "{ element: 'v128', initial: 1 }",
  "{ element: 'anyref', initial: 1 }", "{ element: 'eqref', initial: 1 }", "{ element: 'nullfuncref', initial: 1 }", "{ element: 'ANYFUNC', initial: 1 }", "{ element: '', initial: 1 }", "{ element: undefined, initial: 1 }", "{ element: 1, initial: 1 }", "{ element: null, initial: 1 }",
  "{ initial: 1 }", "{ element: 'anyfunc' }", "{ element: 'anyfunc', initial: -1 }", "{ element: 'anyfunc', initial: 10000000 }", "{ element: 'anyfunc', initial: 10000001 }", "{ element: 'anyfunc', initial: 4294967295 }", "{ element: 'anyfunc', initial: 4294967296 }", "{ element: 'anyfunc', initial: 'x' }", "{ element: 'anyfunc', initial: 1.5 }",
  "{ element: 'anyfunc', initial: 1, maximum: 4294967295 }", "{ element: 'anyfunc', initial: 1, maximum: 4294967296 }", "{ element: 'anyfunc', initial: 1, maximum: -1 }", "{ element: 'anyfunc', initial: 1, maximum: undefined }", "{ element: 'anyfunc', minimum: 3 }", "{ element: 'anyfunc', minimum: 3, initial: 3 }", "{ element: 'anyfunc', initial: 1, index: 'i64' }",
  "undefined", "null", "1", "'anyfunc'", "{}", "[]", "function () {}",
];
for (const spec of tabCases) {
  add(`L(T(() => new WebAssembly.Table(${spec})))`, `L(C(() => { var t = new WebAssembly.Table(${spec}); return t.length + ',' + t.get(0) }))`);
}
for (const spec of ["{ element: 'anyfunc', initial: 1 }", "{ element: 'externref', initial: 1 }"]) {
  add(`L(T(() => new WebAssembly.Table(${spec}, 5)))`, `L(T(() => new WebAssembly.Table(${spec}, null)))`, `L(T(() => new WebAssembly.Table(${spec}, undefined)))`, `L(T(() => new WebAssembly.Table(${spec}, {}).get(0)))`, `L(T(() => new WebAssembly.Table(${spec}, 'x').get(0)))`, `L(T(() => new WebAssembly.Table(${spec}, function () {}).get(0)))`);
}
const tab = "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 2, maximum: 4 }); var x = new WebAssembly.Table({ element: 'externref', initial: 2, maximum: 4 }); var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var f = i.exports.add;";
add(
  "L(T(() => WebAssembly.Table({ element: 'anyfunc', initial: 1 })))",
  tab + "L(t.length); L(t.get(0)); L(t.get(1)); L(x.get(0)); L(x.get(1))",
  tab + "L(T(() => t.get(2))); L(T(() => t.get(-1))); L(T(() => t.get(4294967296))); L(T(() => t.get())); L(T(() => t.get('x'))); L(T(() => t.get(1.5))); L(T(() => t.get(NaN))); L(T(() => t.get({})))",
  tab + "L(T(() => t.get('1'))); L(T(() => t.get(true))); L(T(() => t.get(null))); L(T(() => t.get(1n))); L(T(() => t.get(Symbol())))",
  tab + "L(t.set(0, f)); L(t.get(0) === f); L(t.get(0)(1, 2)); L(t.set(1, null)); L(t.get(1)); L(t.set(0)); L(t.get(0))",
  tab + "L(T(() => t.set(0, 1))); L(T(() => t.set(0, {}))); L(T(() => t.set(0, function () {}))); L(T(() => t.set(0, undefined))); L(T(() => t.set(0, 'x'))); L(T(() => t.set(0, x))); L(T(() => t.set(0, () => 1)))",
  tab + "L(T(() => t.set(2, f))); L(T(() => t.set(-1, f))); L(T(() => t.set(null, f))); L(T(() => t.set('x', f))); L(T(() => t.set(1.5, f))); L(T(() => t.set()))",
  tab + "L(x.set(0, 'x')); L(x.get(0)); L(x.set(1, undefined)); L(x.get(1)); L(x.set(0, null)); L(x.get(0)); L(x.set(0, 5)); L(x.get(0)); L(x.set(1)); L(x.get(1))",
  tab + "var o = {}; x.set(0, o); L(x.get(0) === o); x.set(1, f); L(x.get(1) === f); x.set(1, Symbol.iterator); L(typeof x.get(1)); x.set(0, 1n); L(typeof x.get(0)); x.set(0, NaN); L(x.get(0))",
  tab + "L(t.grow(0)); L(t.length); L(t.grow(1)); L(t.length); L(t.get(2)); L(t.grow(1)); L(t.length); L(T(() => t.grow(1))); L(t.length)",
  tab + "L(T(() => t.grow())); L(T(() => t.grow(-1))); L(T(() => t.grow('x'))); L(T(() => t.grow(1.5))); L(T(() => t.grow(4294967296))); L(T(() => t.grow(3))); L(T(() => t.grow({}))); L(T(() => t.grow(1n)))",
  tab + "L(t.grow(1, f)); L(t.get(2) === f); L(t.get(0)); L(T(() => t.grow(1, 1))); L(t.length); L(t.grow(0, null)); L(x.grow(1, 'y')); L(x.get(2)); L(x.get(0)); L(x.grow(1, undefined)); L(x.get(3)); L(T(() => x.grow(1)))",
  tab + "L(x.grow(1)); L(x.get(2)); L(x.grow(0, 'z')); L(x.get(2))",
  tab + "L(t.length); t.length = 9; L(t.length); L(T(() => { 'use strict'; t.length = 9 })); L(F(WebAssembly.Table.prototype, 'length'))",
  tab + "L(Object.prototype.toString.call(t)); L(String(t)); L(N(t)); L(t instanceof WebAssembly.Table); L(JSON.stringify(t)); L(t.toString === Object.prototype.toString)",
  tab + "L(T(() => WebAssembly.Table.prototype.get.call({}, 0))); L(T(() => WebAssembly.Table.prototype.set.call(1, 0, null))); L(T(() => WebAssembly.Table.prototype.grow.call(null, 1))); L(T(() => WebAssembly.Table.prototype.length))",
  tab + "var g = new WebAssembly.Instance(new WebAssembly.Module(IDT(0x7f))).exports.f; t.set(0, g); L(t.get(0) === g); L(t.get(0).length); L(t.get(0).name); L(t.get(0)(5))",
  tab + "var j = new WebAssembly.Instance(new WebAssembly.Module(ADD)); t.set(0, j.exports.add); L(t.get(0) === f); L(t.get(0)(1, 1))",
  tab + "L(T(() => t.type())); L(typeof t.type)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TAB)); L(i.exports.tab.length); L(i.exports.tab.get(0)); L(i.exports.tab.get(1)); L(T(() => i.exports.tab.grow(1))); L(i.exports.tab.length); L(T(() => i.exports.tab.grow(1)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([], [], [0x41, 0, 0x11, 0, 0]))); L(T(() => i.exports.f())); L(i.exports.tab.length)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f], [], [0x20, 0, 0x11, 0, 0]))); L(T(() => i.exports.f(0))); L(T(() => i.exports.f(1))); L(T(() => i.exports.f(2))); L(T(() => i.exports.f(-1)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([], [], [0x41, 0, 0x11, 0, 0]))); i.exports.tab.set(0, i.exports.f); L(T(() => i.exports.f()))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([], [], [0x41, 0, 0x11, 0, 0]))); var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)); i.exports.tab.set(0, a.exports.add); L(T(() => i.exports.f()))",
  "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1 }); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: t } }); L('ok')",
  "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 0 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: t } })))",
  "var t = new WebAssembly.Table({ element: 'externref', initial: 1 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: t } })))",
  "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1, maximum: 9 }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: t } })))"
);

// Global.
const gTypes = [["i32", ["0", "1", "-1", "2147483648", "4294967296", "1.9", "'12'", "NaN", "null", "undefined", "true", "{}", "1n", "Symbol()", "'x'"]], ["i64", ["0n", "1n", "-1n", "2n**63n", "2n**64n", "0", "1", "'12'", "'x'", "null", "undefined", "true", "{}", "1.5", "NaN"]], ["f32", ["0", "1.5", "0.1", "3.5e38", "1e39", "NaN", "-0", "'2.5'", "null", "undefined", "{}", "1n"]], ["f64", ["0", "1.5", "0.1", "1e308", "NaN", "-0", "Infinity", "'2.5'", "null", "undefined", "{}", "1n"]], ["externref", ["0", "'s'", "null", "undefined", "{}", "1n", "Symbol.iterator", "function () {}"]], ["anyfunc", ["null", "undefined", "0", "{}", "function () {}", "'x'"]], ["funcref", ["null", "0"]], ["anyref", ["0", "null", "{}", "undefined"]], ["eqref", ["0", "null"]], ["i31ref", ["0", "null"]], ["v128", ["0"]], ["i8", ["0"]], ["bool", ["0"]], ["x", ["0"]]];
for (const [type, values] of gTypes) {
  add(`L(T(() => new WebAssembly.Global({ value: '${type}' }))); L(T(() => new WebAssembly.Global({ value: '${type}', mutable: true })))`);
  add(`var g; L(T(() => { g = new WebAssembly.Global({ value: '${type}', mutable: true }); return R(g.value) })); L(g ? typeof g.value : 'x')`);
  for (const v of values) {
    add(`L(C(() => new WebAssembly.Global({ value: '${type}' }, ${v}).value))`);
    add(`L(C(() => { var g = new WebAssembly.Global({ value: '${type}', mutable: true }, ${v}); g.value = ${v}; return g.value }))`);
  }
}
const gl = "var g = new WebAssembly.Global({ value: 'i32', mutable: true }, 7); var h = new WebAssembly.Global({ value: 'i32' }, 7);";
add(
  "L(T(() => new WebAssembly.Global()))", "L(T(() => new WebAssembly.Global(1)))", "L(T(() => new WebAssembly.Global({})))", "L(T(() => new WebAssembly.Global({ mutable: true })))", "L(T(() => new WebAssembly.Global(null)))",
  "L(T(() => WebAssembly.Global({ value: 'i32' })))", "L(T(() => new WebAssembly.Global({ value: 1 })))", "L(T(() => new WebAssembly.Global({ value: 'I32' })))", "L(T(() => new WebAssembly.Global({ value: { toString() { return 'i32' } } }, 3).value))",
  "L(T(() => new WebAssembly.Global({ value: 'i32', mutable: 'yes' }).value)); L(T(() => new WebAssembly.Global({ value: 'i32', mutable: 0 }, 1).value = 2)); L(T(() => new WebAssembly.Global({ value: 'i32', mutable: {} }, 1).value = 2))",
  gl + "L(g.value); g.value = 9; L(g.value); L(g.valueOf()); L(h.valueOf()); L(typeof g.valueOf); L(g.valueOf === h.valueOf)",
  gl + "L(T(() => { h.value = 1 })); L(T(() => { 'use strict'; h.value = 1 })); L(h.value)",
  gl + "L(T(() => { 'use strict'; g.value = 'x' })); L(g.value); L(T(() => { g.value = 1n })); L(T(() => { g.value = Symbol() })); L(g.value)",
  gl + "g.value = 2147483648; L(g.value); g.value = 4294967297; L(g.value); g.value = 1.9; L(g.value); g.value = -1.9; L(g.value); g.value = '5'; L(g.value); g.value = null; L(g.value)",
  gl + "L(T(() => { g.value = undefined })); L(g.value); L(T(() => { g.value = {} })); L(g.value); L(T(() => { g.value = { valueOf() { return 11 } } })); L(g.value)",
  gl + "L(Object.prototype.toString.call(g)); L(String(g)); L(N(g)); L(g instanceof WebAssembly.Global); L(JSON.stringify(g)); L(g + 1)",
  gl + "L(T(() => g.value = undefined)); L(T(() => WebAssembly.Global.prototype.valueOf.call({}))); L(T(() => WebAssembly.Global.prototype.valueOf.call(g))); L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype, 'value').get.call(1)))",
  gl + "L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype, 'value').set.call(g, 5))); L(g.value); L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype, 'value').set.call(g))); L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype, 'value').set.call({}, 1)))",
  gl + "L(T(() => g.type())); L(typeof g.type)",
  "var g = new WebAssembly.Global({ value: 'i64', mutable: true }, 5n); g.value = 2n ** 63n; L(g.value + 'n'); g.value = -(2n ** 63n) - 1n; L(g.value + 'n'); L(T(() => { g.value = 1 })); g.value = true === 1 ? 0n : '77'; L(g.value + 'n')",
  "var g = new WebAssembly.Global({ value: 'f32', mutable: true }, 0.1); L(g.value); g.value = 16777217; L(g.value); g.value = 1e-46; L(g.value); g.value = -1e40; L(g.value); L(Object.is(new WebAssembly.Global({ value: 'f32' }, -0).value, -0))",
  "var g = new WebAssembly.Global({ value: 'f64', mutable: true }, NaN); L(g.value); g.value = -0; L(Object.is(g.value, -0)); g.value = '1e3'; L(g.value)",
  "var o = {}; var g = new WebAssembly.Global({ value: 'externref', mutable: true }, o); L(g.value === o); g.value = undefined; L(g.value); g.value = null; L(g.value); L(new WebAssembly.Global({ value: 'externref' }).value)",
  "var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var g = new WebAssembly.Global({ value: 'anyfunc', mutable: true }, a.exports.add); L(g.value === a.exports.add); g.value = null; L(g.value); L(T(() => { g.value = () => 1 })); L(T(() => { g.value = 5 }))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GLB)); L(i.exports.g.value); L(T(() => { i.exports.g.value = 1 })); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GX(0x7f, true, [0x41, 5]))); L(i.exports.g.value); i.exports.g.value = 8; L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GX(0x7e, false, [0x42, 5]))); L(typeof i.exports.g.value); L(String(i.exports.g.value)); L(Object.prototype.toString.call(i.exports.g))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GX(0x7d, false, [0x43, 0, 0, 0xc0, 0x3f]))); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GX(0x7c, false, [0x44, 0, 0, 0, 0, 0, 0, 0xf8, 0x3f]))); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GX(0x6f, false, [0xd0, 0x6f]))); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GX(0x70, false, [0xd0, 0x70]))); L(i.exports.g.value)",
  "var g = new WebAssembly.Global({ value: 'i32' }, 3); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: g } }); L('ok')",
  "var g = new WebAssembly.Global({ value: 'i32', mutable: true }, 3); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: g } })))",
  "var g = new WebAssembly.Global({ value: 'f32' }, 3); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: g } })))",
  "var g = new WebAssembly.Global({ value: 'i64' }, 3n); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: g } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: 3 } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: 3.5 } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: 2147483648 } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: 3n } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: '3' } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: undefined } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: null } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: {} } })))"
);

// Tag e Exception.
const tg = "var tag = new WebAssembly.Tag({ parameters: ['i32', 'f64'] });";
add(
  "L(typeof WebAssembly.Tag); L(typeof WebAssembly.Exception); L(WebAssembly.Tag.length); L(WebAssembly.Exception.length)",
  "L(T(() => new WebAssembly.Tag()))", "L(T(() => new WebAssembly.Tag({})))", "L(T(() => new WebAssembly.Tag(1)))", "L(T(() => new WebAssembly.Tag({ parameters: 1 })))", "L(T(() => new WebAssembly.Tag({ parameters: ['x'] })))",
  "L(T(() => new WebAssembly.Tag({ parameters: [] })))", "L(T(() => new WebAssembly.Tag({ parameters: ['i32', 'i64', 'f32', 'f64', 'externref', 'anyfunc'] })))", "L(T(() => new WebAssembly.Tag({ parameters: ['v128'] })))", "L(T(() => new WebAssembly.Tag({ parameters: [1] })))",
  "L(T(() => new WebAssembly.Tag({ parameters: ['i32'], results: [] })))", "L(T(() => new WebAssembly.Tag({ parameters: null })))", "L(T(() => new WebAssembly.Tag({ parameters: { length: 1, 0: 'i32' } })))",
  "L(T(() => WebAssembly.Tag({ parameters: [] })))",
  tg + "L(Object.prototype.toString.call(tag)); L(String(tag)); L(N(tag)); L(tag instanceof WebAssembly.Tag); L(JSON.stringify(tag))",
  tg + "L(T(() => new WebAssembly.Exception(tag, [1, 2.5]))); L(T(() => new WebAssembly.Exception(tag, [1]))); L(T(() => new WebAssembly.Exception(tag, [1, 2, 3]))); L(T(() => new WebAssembly.Exception(tag)))",
  tg + "L(T(() => new WebAssembly.Exception())); L(T(() => new WebAssembly.Exception({}, []))); L(T(() => new WebAssembly.Exception(tag, 1))); L(T(() => new WebAssembly.Exception(tag, null))); L(T(() => WebAssembly.Exception(tag, [1, 2])))",
  tg + "var e = new WebAssembly.Exception(tag, [1, 2.5]); L(e.is(tag)); L(e.getArg(tag, 0)); L(e.getArg(tag, 1)); L(T(() => e.getArg(tag, 2))); L(T(() => e.getArg(tag, -1))); L(T(() => e.getArg(tag))); L(T(() => e.getArg({}, 0))); L(T(() => e.is({}))); L(T(() => e.is()))",
  tg + "var t2 = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); var e = new WebAssembly.Exception(tag, [1, 2.5]); L(e.is(t2)); L(T(() => e.getArg(t2, 0)))",
  tg + "var e = new WebAssembly.Exception(tag, [1, 2.5]); L(Object.prototype.toString.call(e)); L(String(e)); L(N(e)); L(e instanceof WebAssembly.Exception); L(e instanceof Error); L(typeof e.stack); L(Object.getPrototypeOf(e) === WebAssembly.Exception.prototype)",
  tg + "var e = new WebAssembly.Exception(tag, [1, 2.5], { traceStack: true }); L(typeof e.stack); L(typeof e.stack === 'string' ? e.stack.length > 0 : 'x'); var f = new WebAssembly.Exception(tag, [1, 2.5], { traceStack: false }); L(f.stack); var g = new WebAssembly.Exception(tag, [1, 2.5]); L(g.stack)",
  tg + "L(T(() => new WebAssembly.Exception(tag, [1, 2.5], 1))); L(T(() => new WebAssembly.Exception(tag, [1, 2.5], null))); L(T(() => new WebAssembly.Exception(tag, [1, 2.5], {})))",
  "var tag = new WebAssembly.Tag({ parameters: ['i64'] }); L(T(() => new WebAssembly.Exception(tag, [1]))); var e = new WebAssembly.Exception(tag, [5n]); L(typeof e.getArg(tag, 0)); L(String(e.getArg(tag, 0))); L(T(() => new WebAssembly.Exception(tag, ['7'])))",
  "var tag = new WebAssembly.Tag({ parameters: ['f32'] }); var e = new WebAssembly.Exception(tag, [0.1]); L(e.getArg(tag, 0)); var t2 = new WebAssembly.Tag({ parameters: ['externref'] }); var o = {}; var f = new WebAssembly.Exception(t2, [o]); L(f.getArg(t2, 0) === o)",
  "var tag = new WebAssembly.Tag({ parameters: [] }); var e = new WebAssembly.Exception(tag, []); L(e.is(tag)); L(T(() => e.getArg(tag, 0))); L(T(() => new WebAssembly.Exception(tag, [1])))",
  "var tag = new WebAssembly.Tag({ parameters: ['anyfunc'] }); L(T(() => new WebAssembly.Exception(tag, [1]))); var e = new WebAssembly.Exception(tag, [null]); L(e.getArg(tag, 0))",
  "class X extends WebAssembly.Exception {}; var tag = new WebAssembly.Tag({ parameters: [] }); var e = new X(tag, []); L(e instanceof X); L(e.is(tag))",
  tg + "L(T(() => WebAssembly.Exception.prototype.is.call({}, tag))); L(T(() => WebAssembly.Exception.prototype.getArg.call({}, tag, 0))); L(T(() => WebAssembly.Tag.prototype.type.call({})))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TAGEXP)); L(i.exports.e instanceof WebAssembly.Tag); L(Object.prototype.toString.call(i.exports.e)); L(T(() => new WebAssembly.Exception(i.exports.e, [])).length)",
  "var tag = new WebAssembly.Tag({ parameters: [] }); var i = new WebAssembly.Instance(new WebAssembly.Module(TAGIMP), { m: { e: tag } }); L(i.exports === undefined)",
  "var tag = new WebAssembly.Tag({ parameters: ['i32'] }); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(TAGIMP), { m: { e: tag } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(TAGIMP), { m: { e: {} } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(TAGIMP), { m: { e: 1 } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(TAGIMP), { m: {} })))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TAGEXP)); var j = new WebAssembly.Instance(new WebAssembly.Module(TAGIMP), { m: { e: i.exports.e } }); L('ok')",
  "var tag = new WebAssembly.Tag({ parameters: [] }); var m = new WebAssembly.Module(FX([], [], [0x08, 0])); L(T(() => m))",
  "var tag = new WebAssembly.Tag({ parameters: [] }); L(typeof WebAssembly.JSTag); L(T(() => WebAssembly.JSTag instanceof WebAssembly.Tag))",
  "L(Object.prototype.toString.call(WebAssembly.RuntimeError.prototype)); L(new WebAssembly.RuntimeError('x') instanceof Error); L(new WebAssembly.RuntimeError('x').message); L(String(new WebAssembly.RuntimeError('x')))"
);

// Erros: construtores.
for (const name of ["CompileError", "LinkError", "RuntimeError"]) {
  const C = `WebAssembly.${name}`;
  add(
    `var e = new ${C}('m'); L(e.name); L(e.message); L(String(e)); L(e instanceof Error); L(e instanceof ${C}); L(N(e)); L(F(e, 'message')); L(F(e, 'name')); L(typeof e.stack)`,
    `var e = ${C}('m'); L(e.name); L(e.message); L(e instanceof ${C})`,
    `L(N(new ${C}())); L(new ${C}().message); L(String(new ${C}())); L(new ${C}(undefined).hasOwnProperty('message')); L(new ${C}(1).message); L(new ${C}(null).message); L(new ${C}({ toString() { return 'ts' } }).message)`,
    `var e = new ${C}('a', { cause: 7 }); L(e.cause); L(N(e)); var f = new ${C}('a', {}); L(f.hasOwnProperty('cause')); var g = new ${C}('a', { cause: undefined }); L(g.hasOwnProperty('cause'))`,
    `L(${C}.prototype.name); L(F(${C}.prototype, 'name')); L(F(${C}.prototype, 'message')); L(${C}.prototype.message === ''); L(${C}.prototype.hasOwnProperty('toString'))`,
    `class X extends ${C} {}; var e = new X('q'); L(e instanceof X); L(e instanceof ${C}); L(e.name); L(String(e)); L(Object.prototype.toString.call(e))`,
    `var e = new ${C}('s'); L(Object.prototype.toString.call(e)); L(Error.prototype.toString.call(e)); L(e.toString === Error.prototype.toString); L(Object.getPrototypeOf(${C}) === Error)`,
    `L(Reflect.construct(${C}, ['x'], Object).message === undefined); L(Object.getPrototypeOf(Reflect.construct(${C}, ['x'], Object)) === Object.prototype)`
  );
}

// compile e instantiate.
add(
  "P(WebAssembly.compile(ADD), 'c')", "P(WebAssembly.instantiate(ADD), 'i')", "P(WebAssembly.instantiate(new WebAssembly.Module(ADD)), 'i')",
  "L(WebAssembly.compile(ADD) instanceof Promise); L(WebAssembly.instantiate(ADD) instanceof Promise); L(Object.getPrototypeOf(WebAssembly.compile(ADD)) === Promise.prototype)",
  "P(WebAssembly.compile(EMPTY), 'c')", "P(WebAssembly.compile(ADD.slice(0, 5)), 'c')", "P(WebAssembly.compile(new Uint8Array(0)), 'c')", "P(WebAssembly.compile(new Uint8Array([0, 97, 115, 109, 2, 0, 0, 0])), 'c')",
  "WebAssembly.instantiate(ADD).then(function (r) { L(N(r)); L(r.module instanceof WebAssembly.Module); L(r.instance instanceof WebAssembly.Instance); L(r.instance.exports.add(2, 3)); L(Object.keys(r).join(',')); L(Object.getPrototypeOf(r) === Object.prototype) })",
  "WebAssembly.instantiate(new WebAssembly.Module(ADD)).then(function (r) { L(r instanceof WebAssembly.Instance); L(r.exports.add(2, 3)); L(N(r)) })",
  "WebAssembly.instantiate(ADD.buffer.slice(0)).then(function (r) { L(Object.keys(r).join(',')) })",
  "WebAssembly.instantiate(ADD, undefined).then(function (r) { L('ok') })", "P(WebAssembly.instantiate(ADD, null), 'n')", "P(WebAssembly.instantiate(ADD, 1), 'n')", "P(WebAssembly.instantiate(IMP), 'n')", "P(WebAssembly.instantiate(IMP, {}), 'n')", "P(WebAssembly.instantiate(IMP, { m: {} }), 'n')", "P(WebAssembly.instantiate(IMP, { m: { f: 1 } }), 'n')", "P(WebAssembly.instantiate(IMP, { m: { f: function (x) { return x } } }), 'n')",
  "P(WebAssembly.instantiate(new WebAssembly.Module(IMP)), 'n')", "P(WebAssembly.instantiate(new WebAssembly.Module(IMP), {}), 'n')", "P(WebAssembly.instantiate(new WebAssembly.Module(IMP), { m: { f: function (x) { return x } } }), 'n')",
  "P(WebAssembly.instantiate(new WebAssembly.Module(ADD), 1), 'n')", "P(WebAssembly.instantiate(new WebAssembly.Module(ADD), 'x'), 'n')",
  "P(WebAssembly.instantiate(TRAP), 'n')", "P(WebAssembly.instantiate(START), 'n')",
  "P(WebAssembly.instantiate(mk(sec(1, [1, 0x60, 0, 0]), sec(3, [1, 0]), sec(8, [0]), sec(10, [1, 3, 0, 0x00, 0x0b]))), 's')",
  "var r = []; WebAssembly.instantiate(mk(sec(1, [1, 0x60, 0, 0]), sec(2, [1].concat(str('m'), str('f'), [0, 0])), sec(8, [0]))).then(function (x) { return WebAssembly.instantiate(x.module, { m: { f: function () { L('start') } } }) }).then(function (i) { L(D(i)) })",
  "var m = new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(2, [1].concat(str('m'), str('f'), [0, 0])), sec(8, [0]))); L(T(() => new WebAssembly.Instance(m, { m: { f: function () { throw new TypeError('in start') } } })))",
  "var m = new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(2, [1].concat(str('m'), str('f'), [0, 0])), sec(8, [0]))); P(WebAssembly.instantiate(m, { m: { f: function () { throw new TypeError('in start') } } }), 'st')",
  "P(WebAssembly.compile(ADD).then(function (m) { return WebAssembly.instantiate(m) }), 'chain')",
  "P(WebAssembly.compile(ADD).then(function (m) { return [WebAssembly.Module.exports(m).length] }), 'ex')",
  "WebAssembly.compile(ADD).then(function (m) { L(m instanceof WebAssembly.Module); L(Object.prototype.toString.call(m)) })",
  "WebAssembly.compile(ADD).then(function () { L('a') }); Promise.resolve().then(function () { L('b') }); L('sync')",
  "WebAssembly.compile(new Uint8Array(0)).catch(function (e) { L(e.name); L(e.message); L(e instanceof WebAssembly.CompileError) })",
  "WebAssembly.compile(1).catch(function (e) { L(e.name); L(e.message) })",
  "WebAssembly.instantiate(IMP, {}).catch(function (e) { L(e.name); L(e.message); L(e instanceof WebAssembly.LinkError) })",
  "WebAssembly.instantiate(IMP, { m: { f: 1 } }).catch(function (e) { L(e.name); L(e.message); L(e instanceof WebAssembly.LinkError) })",
  "WebAssembly.instantiate(IMP).catch(function (e) { L(e.name); L(e.message) })",
  "WebAssembly.instantiate(new Uint8Array(0)).catch(function (e) { L(e.name); L(e.message) })",
  "var p = WebAssembly.instantiate(ADD); var q = WebAssembly.instantiate(ADD); L(p === q); P(p, 'p')",
  "var calls = 0; var b = ADD.slice(); var p = WebAssembly.compile(b); b[0] = 9; P(p, 'p')",
  "var b = ADD.slice(); var p = WebAssembly.instantiate(b); b[0] = 9; P(p, 'p')",
  "var p = WebAssembly.compile(ADD); L(typeof p.then); L(typeof p.catch); P(p, 'x'); P(p, 'y')",
  "WebAssembly.compile(ADD).then(function (a) { return WebAssembly.compile(ADD).then(function (b) { L(a === b) }) })",
  "WebAssembly.validate(ADD) && WebAssembly.compile(ADD).then(function () { L(1) }); Promise.resolve().then(function () { L(3) }).then(function () { L(4) }).then(function () { L(5) }).then(function () { L(6) })",
  "L(T(() => WebAssembly.compile.call(null, ADD) instanceof Promise)); L(T(() => new WebAssembly.compile(ADD))); L(T(() => new WebAssembly.instantiate(ADD)))",
  "L(T(() => WebAssembly.compile.call({}, ADD) instanceof Promise)); P(WebAssembly.compile.call(undefined, ADD), 'u')",
  "var o = { then: function () {} }; P(WebAssembly.compile(ADD), 'o'); L(typeof o.then)",
  "P(WebAssembly.instantiate(ADD, { get m() { throw new Error('getter') } }), 'g')", "P(WebAssembly.instantiate(IMP, { get m() { throw new Error('getter') } }), 'g')", "P(WebAssembly.instantiate(IMP, new Proxy({}, { get() { throw new RangeError('px') } })), 'g')",
  "var order = []; var p = WebAssembly.instantiate(IMP, { get m() { order.push('get'); return { f: function (x) { return x } } } }); order.push('after'); p.then(function () { L(order.join(',')) })",
  "var order = []; var m = new WebAssembly.Module(IMP); var p = WebAssembly.instantiate(m, { get m() { order.push('get'); return { f: function (x) { return x } } } }); order.push('after'); p.then(function () { L(order.join(',')) })",
  "var order = []; try { new WebAssembly.Instance(new WebAssembly.Module(IMP), { get m() { order.push('get'); return { f: function (x) { return x } } } }) } catch (e) {}; L(order.join(','))",
  "var order = []; var o = { get m() { order.push('m'); return { get f() { order.push('f'); return function (x) { return x } } } } }; new WebAssembly.Instance(new WebAssembly.Module(IMP), o); L(order.join(','))"
);

// importObject.
const imp = "var m = new WebAssembly.Module(IMP);";
const impVals = ["undefined", "null", "1", "'x'", "true", "{}", "[]", "function () {}", "() => 1", "class A {}", "Symbol()", "1n", "new Proxy({}, {})", "new Proxy(function () {}, {})", "async function () {}", "function* () {}", "Math.max", "Object", "{ call() {} }", "function () { return 1 }.bind(null)", "new WebAssembly.Memory({ initial: 1 })", "new WebAssembly.Global({ value: 'i32' }, 1)"];
for (const v of impVals) add(imp + `L(T(() => new WebAssembly.Instance(m, { m: { f: ${v} } })))`);
const modVals = ["undefined", "null", "1", "'x'", "true", "[]", "function () {}", "Symbol()", "new Proxy({}, {})", "new Proxy(function () {}, {})", "{ f: function () {} }", "[function () {}]", "Object.create({ f: function (x) { return x } })", "new (class { get f() { return function (x) { return x + 1 } } })()"];
for (const v of modVals) add(imp + `L(T(() => new WebAssembly.Instance(m, { m: ${v} })))`);
add(
  imp + "L(T(() => new WebAssembly.Instance(m, { x: { f: function () {} } })))",
  imp + "L(T(() => new WebAssembly.Instance(m, { m: { g: function () {} } })))",
  imp + "L(T(() => new WebAssembly.Instance(m, [])))", imp + "L(T(() => new WebAssembly.Instance(m, function () {})))", imp + "L(T(() => new WebAssembly.Instance(m, new Proxy({}, {}))))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return x + 1 } } }); L(i.exports.g(1)); L(i.exports.g(2147483647)); L(i.exports.g('3')); L(i.exports.g())",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return 'x' } } }); L(i.exports.g(1))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return 1.9 } } }); L(i.exports.g(1))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return 4294967297 } } }); L(i.exports.g(1))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return undefined } } }); L(i.exports.g(1))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return 1n } } }); L(T(() => i.exports.g(1)))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return Symbol() } } }); L(T(() => i.exports.g(1)))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return { valueOf() { return 8 } } } } }); L(i.exports.g(1))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function (x) { return { valueOf() { throw new SyntaxError('vo') } } } } }); L(T(() => i.exports.g(1)))",
  imp + "var args; var i = new WebAssembly.Instance(m, { m: { f: function () { args = arguments.length + ':' + typeof this; return 0 } } }); i.exports.g(1); L(args)",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function () { 'use strict'; L(this === undefined); return 0 } } }); i.exports.g(1)",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: function () { return 0 } } }); var j = new WebAssembly.Instance(m, { m: { f: i.exports.g } }); L(j.exports.g(5))",
  imp + "var i = new WebAssembly.Instance(m, { m: { f: Math.abs } }); L(i.exports.g(-5)); var k = new WebAssembly.Instance(m, { m: { f: parseInt } }); L(k.exports.g(7))",
  imp + "var f = function (x) { return x }; var i = new WebAssembly.Instance(m, { m: { f: f } }); var j = new WebAssembly.Instance(m, { m: { f: f } }); L(i.exports.g === j.exports.g)",
  imp + "var f = function (x) { return x }; var i = new WebAssembly.Instance(new WebAssembly.Module(REEXP([0x7f], [0x7f])), { m: { f: f } }); L(i.exports.f === f); L(typeof i.exports.f); L(i.exports.f(4)); L(i.exports.f.name); L(i.exports.f.length)",
  "var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var i = new WebAssembly.Instance(new WebAssembly.Module(REEXP([0x7f, 0x7f], [0x7f])), { m: { f: a.exports.add } }); L(i.exports.f === a.exports.add); L(i.exports.f(3, 4))",
  "var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(REEXP([0x7f], [0x7f])), { m: { f: a.exports.add } })))",
  "var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(REEXP([0x7e, 0x7e], [0x7e])), { m: { f: a.exports.add } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), { m: {} })))",
  "var o = { m: { f: function () {}, t: new WebAssembly.Table({ element: 'anyfunc', initial: 1 }), mem: new WebAssembly.Memory({ initial: 1, maximum: 2 }), g: new WebAssembly.Global({ value: 'i32', mutable: true }, 1) } }; var i = new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), o); L(N(i.exports))",
  "var o = { m: { f: function () {}, t: new WebAssembly.Table({ element: 'anyfunc', initial: 1 }), mem: new WebAssembly.Memory({ initial: 1, maximum: 2 }), g: new WebAssembly.Global({ value: 'i32' }, 1) } }; L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), o)))",
  "var o = { m: { f: function () {}, t: new WebAssembly.Table({ element: 'anyfunc', initial: 1 }), mem: new WebAssembly.Memory({ initial: 1 }), g: new WebAssembly.Global({ value: 'i32', mutable: true }, 1) } }; L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), o)))",
  "var o = { m: { f: function () {}, t: {}, mem: new WebAssembly.Memory({ initial: 1, maximum: 2 }), g: new WebAssembly.Global({ value: 'i32', mutable: true }, 1) } }; L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), o)))",
  "var o = { m: { f: function () {}, t: new WebAssembly.Table({ element: 'anyfunc', initial: 1 }), mem: {}, g: new WebAssembly.Global({ value: 'i32', mutable: true }, 1) } }; L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), o)))",
  "var o = { m: { f: function () {}, t: new WebAssembly.Table({ element: 'anyfunc', initial: 1 }), mem: new WebAssembly.Memory({ initial: 1, maximum: 2 }), g: 1 } }; L(T(() => new WebAssembly.Instance(new WebAssembly.Module(ALLIMP), o)))"
);

// Exports de função: nome, length, conversões.
for (const [i, t] of [[0, 0x7f], [1, 0x7e], [2, 0x7d], [3, 0x7c]]) {
  const type = ["i32", "i64", "f32", "f64"][i];
  const vals = i === 1
    ? ["0n", "1n", "-1n", "2n**63n", "2n**64n", "2n**64n+5n", "-(2n**63n)", "0", "1", "'12'", "'0x10'", "'abc'", "''", "null", "undefined", "true", "false", "{}", "[]", "1.5", "NaN", "Symbol()", "{ valueOf() { return 3n } }", "{ valueOf() { return 3 } }", "[5n]", "'  7  '", "'1e3'"]
    : ["0", "1", "-1", "2147483647", "2147483648", "4294967295", "4294967296", "-2147483649", "1.5", "-1.5", "0.5", "NaN", "Infinity", "-Infinity", "'7'", "'0x10'", "''", "'abc'", "'  8 '", "true", "false", "null", "undefined", "[]", "[5]", "[1, 2]", "({})", "({ valueOf() { return 3 } })", "({ valueOf() { throw new RangeError('vo') } })", "({ toString() { return '9' } })", "1e20", "-0", "0.1", "3.4028235e38", "3.5e38", "1e40", "1.401298464324817e-45", "5n", "-5n", "2n**64n", "Symbol('s')", "new Date(5)", "'1_0'", "'Infinity'"];
  for (const v of vals) add(`var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(${t}))).exports.f; L(C(() => f(${v})))`);
  add(
    `var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(${t}))).exports.f; L(f.length); L(f.name); L(C(() => f())); L(C(() => f(${i === 1 ? "1n" : "1"}, 2, 3))); L(typeof f)`,
    `var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(${t}))).exports.f; L(C(() => f.call(null))); L(C(() => f.apply(null, [${i === 1 ? "4n" : "4"}]))); L(C(() => new f(${i === 1 ? "4n" : "4"})))`
  );
}
for (const t of [0x6f, 0x70]) {
  const vals = t === 0x6f ? ["0", "'s'", "null", "undefined", "{}", "1n", "Symbol.iterator", "NaN", "function () {}"] : ["null", "undefined", "0", "{}", "function () {}", "'x'"];
  for (const v of vals) add(`var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(${t}))).exports.f; L(C(() => { var r = f(${v}); return typeof r + ':' + (r === ${v}) }))`);
  add(`var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(${t}))).exports.f; L(f.length); L(C(() => f()))`);
}
add(
  "var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(0x70))).exports.f; var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)).exports.add; L(f(a) === a); L(T(() => f(() => 1)))",
  "var a = new WebAssembly.Instance(new WebAssembly.Module(ADD)).exports.add; var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f, 0x7f, 0x7f], [0x7f], [0x20, 0, 0x20, 1, 0x6a, 0x20, 2, 0x6a]))).exports.f; L(f(1, 2, 3)); L(f(1, 2)); L(f(1)); L(f()); L(f.length)",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f, 0x7e], [0x7e], [0x20, 1]))).exports.f; L(C(() => f(1, 2n))); L(C(() => f(1))); L(C(() => f(1, 2)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7e], [0x7f], [0x20, 0, 0xa7]))).exports.f; L(C(() => f(4294967297n))); L(C(() => f(-1n)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f], [0x7e], [0x20, 0, 0xad]))).exports.f; L(C(() => f(-1))); L(C(() => f(5)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7d], [0x7c], [0x20, 0, 0xbb]))).exports.f; L(C(() => f(0.1))); L(C(() => f(16777217)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7c], [0x7d], [0x20, 0, 0xb6]))).exports.f; L(C(() => f(0.1))); L(C(() => f(1e300)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([], [0x7f, 0x7f], [0x41, 1, 0x41, 2]))).exports.f; L(C(() => JSON.stringify(f()))); L(C(() => Array.isArray(f()))); L(C(() => Object.getPrototypeOf(f()) === Array.prototype))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([], [0x7f, 0x7e, 0x7c], [0x41, 1, 0x42, 2, 0x44, 0, 0, 0, 0, 0, 0, 0xf8, 0x3f]))).exports.f; L(C(() => f().map(R).join(',')))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([], [], []))).exports.f; L(C(() => f())); L(f() === undefined)",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f, 0x7f], [0x7f, 0x7f], [0x20, 1, 0x20, 0]))).exports.f; L(C(() => JSON.stringify(f(1, 2))))"
);
// Funções com muitos parâmetros e mistura de tipos.
for (const n of [1, 2, 4, 8, 16, 32]) {
  add(`var f = new WebAssembly.Instance(new WebAssembly.Module(FX(Array(${n}).fill(0x7f), [0x7f], [0x20, ${n - 1}]))).exports.f; L(f.length); L(f(${Array.from({ length: n }, (_, k) => k + 1).join(",")})); L(f())`);
}

// Traps.
const trapBodies = {
  unreachable: [[], [], [0x00]],
  div_zero: [[], [0x7f], [0x41, 1, 0x41, 0, 0x6d]],
  div_u_zero: [[], [0x7f], [0x41, 1, 0x41, 0, 0x6e]],
  rem_zero: [[], [0x7f], [0x41, 1, 0x41, 0, 0x6f]],
  rem_u_zero: [[], [0x7f], [0x41, 1, 0x41, 0, 0x70]],
  div_overflow: [[], [0x7f], [0x41, 0x80, 0x80, 0x80, 0x80, 0x78, 0x41, 0x7f, 0x6d]],
  rem_overflow: [[], [0x7f], [0x41, 0x80, 0x80, 0x80, 0x80, 0x78, 0x41, 0x7f, 0x6f]],
  i64_div_zero: [[], [0x7e], [0x42, 1, 0x42, 0, 0x7f]],
  i64_div_u_zero: [[], [0x7e], [0x42, 1, 0x42, 0, 0x80]],
  i64_rem_zero: [[], [0x7e], [0x42, 1, 0x42, 0, 0x81]],
  i64_div_overflow: [[], [0x7e], [0x42, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f, 0x42, 0x7f, 0x7f]],
  trunc_nan: [[], [0x7f], [0x43, 0, 0, 0xc0, 0x7f, 0xa8]],
  trunc_inf: [[], [0x7f], [0x43, 0, 0, 0x80, 0x7f, 0xa8]],
  trunc_big: [[], [0x7f], [0x43, 0, 0, 0, 0x4f, 0xa8]],
  trunc_neg_u: [[], [0x7f], [0x43, 0, 0, 0x80, 0xbf, 0xa9]],
  trunc64_nan: [[], [0x7e], [0x44, 0, 0, 0, 0, 0, 0, 0xf8, 0x7f, 0xb0]],
  trunc64_big: [[], [0x7e], [0x44, 0, 0, 0, 0, 0, 0, 0xe0, 0x43, 0xb0]],
  load_oob: [[], [0x7f], [0x41, 0x80, 0x80, 0x04, 0x28, 2, 0]],
  load_oob_edge: [[], [0x7f], [0x41, 0xfd, 0xff, 0x03, 0x28, 2, 0]],
  load_offset: [[], [0x7f], [0x41, 0, 0x28, 2, 0xff, 0xff, 0x03]],
  store_oob: [[], [], [0x41, 0x80, 0x80, 0x04, 0x41, 1, 0x36, 2, 0]],
  load_neg: [[], [0x7f], [0x41, 0x7f, 0x28, 2, 0]],
  load8_oob: [[], [0x7f], [0x41, 0x80, 0x80, 0x04, 0x2d, 0, 0]],
  load64_oob: [[], [0x7e], [0x41, 0xf9, 0xff, 0x03, 0x29, 3, 0]],
  call_null: [[], [], [0x41, 0, 0x11, 0, 0]],
  call_oob: [[], [], [0x41, 5, 0x11, 0, 0]],
  call_neg: [[], [], [0x41, 0x7f, 0x11, 0, 0]],
  recurse: [[], [], [0x10, 0]],
  recurse_val: [[0x7f], [0x7f], [0x20, 0, 0x41, 1, 0x6a, 0x10, 0]],
  mem_grow_ok: [[], [0x7f], [0x41, 1, 0x40, 0]],
  mem_grow_huge: [[], [0x7f], [0x41, 0x80, 0x80, 0x04, 0x40, 0]],
  table_get_oob: [[], [], [0x41, 9, 0x25, 0, 0x1a]],
  table_set_oob: [[], [], [0x41, 9, 0xd0, 0x70, 0x26, 0]],
  mem_fill_oob: [[], [], [0x41, 0, 0x41, 0, 0x41, 0x81, 0x80, 0x04, 0xfc, 11, 0]],
  mem_copy_oob: [[], [], [0x41, 0, 0x41, 0, 0x41, 0x81, 0x80, 0x04, 0xfc, 10, 0, 0]],
  mem_init_nodata: [[], [], [0x41, 0, 0x41, 0, 0x41, 1, 0xfc, 8, 0, 0]],
  unreachable_after_ret: [[], [0x7f], [0x41, 3, 0x0f, 0x00]],
};
for (const [name, [p, r, body]] of Object.entries(trapBodies)) {
  const mod = `FX(${JSON.stringify(p)}, ${JSON.stringify(r)}, ${JSON.stringify(body)})`;
  add(
    `var i = new WebAssembly.Instance(new WebAssembly.Module(${mod})); L(C(() => i.exports.f(${p.length ? "0" : ""})))`,
    `var i = new WebAssembly.Instance(new WebAssembly.Module(${mod})); try { i.exports.f(${p.length ? "0" : ""}); L('sem trap') } catch (e) { L(e.name); L(e.constructor === WebAssembly.RuntimeError); L(e instanceof Error); L(e.message); L(typeof e.stack); L(N(e)) }`
  );
}
add(
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TRAP)); try { i.exports.f() } catch (e) { L(Object.prototype.toString.call(e)); L(String(e)); L(e.constructor === WebAssembly.RuntimeError); L(Object.getPrototypeOf(e) === WebAssembly.RuntimeError.prototype) }",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TRAP)); for (var k = 0; k < 3; k++) L(T(() => i.exports.f()))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TRAP)); var e1, e2; try { i.exports.f() } catch (e) { e1 = e } try { i.exports.f() } catch (e) { e2 = e } L(e1 === e2)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([], [], [0x10, 0]))); try { i.exports.f() } catch (e) { L(e.name); L(e instanceof RangeError); L(e.message); L(e instanceof WebAssembly.RuntimeError) }",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(FX([], [], [0x10, 0]))); try { i.exports.f() } catch (e) { L(e.constructor === RangeError) } L(T(() => i.exports.f()))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: function (x) { return i.exports.g(x) } } }); L(T(() => i.exports.g(1)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: function (x) { throw 5 } } }); try { i.exports.g(1) } catch (e) { L(e) }",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: function (x) { throw undefined } } }); try { i.exports.g(1); L('no') } catch (e) { L(typeof e) }",
  "var o = { a: 1 }; var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: function (x) { throw o } } }); try { i.exports.g(1) } catch (e) { L(e === o) }",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: function (x) { return i.exports.g(x - 1) } } }); L(T(() => i.exports.g(3)))".replace("return i.exports.g(x - 1)", "return x > 0 ? i.exports.g(x - 1) : 7"),
  "var i = new WebAssembly.Instance(new WebAssembly.Module(START)); L(i.exports.g.value)",
  "var order = []; var m = new WebAssembly.Module(mk(sec(1, [1, 0x60, 0, 0]), sec(2, [1].concat(str('m'), str('f'), [0, 0])), sec(8, [0]))); try { new WebAssembly.Instance(m, { m: { f: function () { order.push('start'); throw new Error('e') } } }) } catch (e) { order.push(e.message) } L(order.join(','))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(0x7f))).exports.f; var big = {}; for (var k = 0; k < 1; k++) big.valueOf = function () { throw new URIError('conv') }; L(T(() => f(big)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(IDT(0x7e))).exports.f; L(T(() => f({ valueOf() { throw new URIError('conv') } }))); L(T(() => f({ toString() { return '12' } })))",
  "var order = []; var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f, 0x7f], [0x7f], [0x20, 0]))).exports.f; f({ valueOf() { order.push('a'); return 1 } }, { valueOf() { order.push('b'); return 2 } }); L(order.join(','))",
  "var order = []; var f = new WebAssembly.Instance(new WebAssembly.Module(FX([0x7f, 0x7f], [0x7f], [0x20, 0]))).exports.f; try { f({ valueOf() { order.push('a'); throw 1 } }, { valueOf() { order.push('b'); return 2 } }) } catch (e) {} L(order.join(','))"
);

// Várias instâncias e interações.
add(
  "var m = new WebAssembly.Module(ADD); var a = new WebAssembly.Instance(m); var b = new WebAssembly.Instance(m); L(a.exports.add === b.exports.add); L(a === b); L(a.exports === b.exports)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(MEM)); var j = new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: i.exports.mem } }); L(i.exports.mem.grow(1)); L(i.exports.mem.buffer.byteLength)",
  "var t = new WebAssembly.Instance(new WebAssembly.Module(TAB)); var u = new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: t.exports.tab } }); L('ok')",
  "var g = new WebAssembly.Instance(new WebAssembly.Module(GX(0x7f, false, [0x41, 9]))); var h = new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: g.exports.g } }); L(g.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); L(i.exports.mem.buffer.byteLength); L(i.exports.t.length); L(i.exports.g.value); L(typeof i.exports.f); L(i.exports.f()); L(i.exports.f.length)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ALLEXP)); i.exports.mem.grow(1); L(i.exports.mem.buffer.byteLength); i.exports.t.grow(1); L(i.exports.t.length); L(T(() => i.exports.t.grow(1)))",
  "L(Object.getOwnPropertyNames(WebAssembly).sort().join(','))", "L(Object.getOwnPropertyNames(WebAssembly.Module).sort().join(','))", "L(Reflect.ownKeys(WebAssembly).map(String).join(','))",
  "L(Reflect.ownKeys(WebAssembly.Memory.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.Table.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.Global.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.Instance.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.Module.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.Tag.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.Exception.prototype).map(String).join(','))",
  "L(Reflect.ownKeys(WebAssembly.CompileError.prototype).map(String).join(','))", "L(Reflect.ownKeys(WebAssembly.LinkError).map(String).join(','))",
  "L(Object.prototype.toString.call(WebAssembly.Memory)); L(Object.prototype.toString.call(WebAssembly.validate)); L(Object.prototype.toString.call(WebAssembly.Module.prototype))",
  "L(globalThis.hasOwnProperty('WebAssembly')); L(F(globalThis, 'WebAssembly'))",
  "L(F(globalThis, 'CompileError')); L(typeof LinkError)"
);

// Validação: módulos inválidos montados de wat (wasm-tools parse não valida), cada um medido por validate, Module,
// compile e instantiate (mensagens exatas do JSC, com o byte e o texto do validador).
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-api-golden-"));
let watCount = 0;
const bytesOf = (wat) => {
  const file = path.join(tmp, `v${watCount++}.wat`);
  fs.writeFileSync(file, wat);
  const parsed = spawnSync("wasm-tools", ["parse", file, "-o", file + ".wasm"], { encoding: "utf8" });
  if (parsed.status !== 0) return null;
  return "new Uint8Array([" + Array.from(fs.readFileSync(file + ".wasm")).join(",") + "])";
};
const invalidWats = [
  "(module (func (result i32)))",
  "(module (func (result i32) i64.const 1))",
  "(module (func (result i32) f32.const 1))",
  "(module (func (param i32) (result i64) local.get 0))",
  "(module (func i32.const 1))",
  "(module (func i32.add))",
  "(module (func (param i32) local.get 0 local.get 0 i64.add drop))",
  "(module (func (result i32) i32.const 1 i32.const 2))",
  "(module (func (param f64) local.get 0 i32.eqz drop))",
  "(module (func br 1))",
  "(module (func block br 2 end))",
  "(module (func loop br_if 3 end))",
  "(module (func i32.const 0 br_if 0))",
  "(module (func (result i32) block (result i32) end))",
  "(module (func (result i32) i32.const 0 if (result i32) i32.const 1 end))",
  "(module (func i32.const 0 if i32.const 1 else end))",
  "(module (func local.get 0 drop))",
  "(module (func (local i32) local.get 1 drop))",
  "(module (func (local i32) i64.const 0 local.set 0))",
  "(module (func call 5))",
  "(module (func (param i32) call 0))",
  "(module (func i32.const 0 call_indirect))",
  "(module (type (func)) (func i32.const 0 call_indirect (type 0)))",
  "(module (table 1 funcref) (func i32.const 0 call_indirect (type 3)))",
  "(module (memory 1) (func i32.const 0 i64.load drop))",
  "(module (func i32.const 0 i32.load drop))",
  "(module (memory 1) (func i32.const 0 i32.load align=8 drop))",
  "(module (memory 1) (func i32.const 0 i32.const 0 i64.store))",
  "(module (func memory.size drop))",
  "(module (memory 1) (func i32.const 0 memory.grow i64.extend_i32_u drop drop))",
  "(module (global i32 (i32.const 0)) (func i32.const 1 global.set 0))",
  "(module (global (mut i32) (i32.const 0)) (func i64.const 1 global.set 0))",
  "(module (func global.get 0 drop))",
  "(module (global i32 (i64.const 0)))",
  "(module (global i32 (global.get 0)))",
  "(module (global i32 (i32.add (i32.const 1) (i32.const 2))))",
  "(module (func (result i32) unreachable i64.const 0 i32.add))",
  "(module (func (result i32) return))",
  "(module (func (result i32) i64.const 0 return))",
  "(module (func (result i32 i32) i32.const 0 return))",
  "(module (func select))",
  "(module (func i32.const 0 i64.const 0 i32.const 0 select drop))",
  "(module (func drop))",
  "(module (func (result i32) block (result i64) i64.const 0 end))",
  "(module (func (result i32) loop (result f32) f32.const 0 end))",
  "(module (func i32.const 0 br_table 0 1))",
  "(module (func block block i32.const 0 br_table 0 1 2 end end))",
  "(module (func (result i32) block (result i32) i32.const 0 i32.const 0 br_table 0 0 end))",
  "(module (start 0))",
  "(module (func $f (param i32)) (start $f))",
  "(module (func $f (result i32) i32.const 0) (start $f))",
  "(module (func) (export \"a\" (func 0)) (export \"a\" (func 0)))",
  "(module (func) (export \"a\" (func 1)))",
  "(module (memory 1) (export \"m\" (memory 1)))",
  "(module (table 1 funcref) (export \"t\" (table 1)))",
  "(module (global i32 (i32.const 0)) (export \"g\" (global 1)))",
  "(module (memory 1) (memory 1))",
  "(module (memory 65537))",
  "(module (memory 2 1))",
  "(module (memory 0 65537))",
  "(module (memory 1 1 shared))",
  "(module (memory 1 shared))",
  "(module (table 2 1 funcref))",
  "(module (table 10000001 funcref))",
  "(module (elem (i32.const 0) 0))",
  "(module (table 1 funcref) (elem (i32.const 0) 0))",
  "(module (table 1 funcref) (func) (elem (i64.const 0) 0))",
  "(module (table 1 funcref) (func) (elem (i32.const 0) 0 1))",
  "(module (table 1 externref) (func) (elem (i32.const 0) func 0))",
  "(module (data (i32.const 0) \"a\"))",
  "(module (memory 1) (data (i64.const 0) \"a\"))",
  "(module (memory 1) (data (global.get 0) \"a\"))",
  "(module (import \"m\" \"f\" (func (type 5))))",
  "(module (type (func (param i32))) (func (type 0)) (func (type 1)))",
  "(module (type (func (result i32 i32))) (func (type 0) i32.const 0))",
  "(module (func (param v128)))",
  "(module (func (result externref) ref.null func))",
  "(module (func (result funcref) ref.null extern))",
  "(module (func (param funcref) (result externref) local.get 0))",
  "(module (func ref.func 0 drop) (func ref.func 7 drop))",
  "(module (func (result i32) ref.null extern ref.is_null))",
  "(module (func i32.const 0 ref.is_null drop))",
  "(module (table 1 funcref) (func i32.const 0 i32.const 0 table.set 0))",
  "(module (table 1 funcref) (func i32.const 0 table.get 1 drop))",
  "(module (table 1 funcref) (table 1 externref) (func (param i32) local.get 0 table.get 1 drop table.copy 0 1))",
  "(module (func (param i32) (result i32) local.get 0 return_call 0 i32.const 0))",
  "(module (type $s (struct (field i32))) (func (result (ref $s)) struct.new $s))",
  "(module (type $s (struct (field i32))) (func (result (ref $s)) i64.const 0 struct.new $s))",
  "(module (type $s (struct (field i32))) (func (param (ref $s)) local.get 0 struct.get $s 1 drop))",
  "(module (type $s (struct (field i32))) (func (param (ref $s)) local.get 0 i32.const 0 struct.set $s 0))",
  "(module (type $s (struct (field (mut i32)))) (func (param (ref $s)) local.get 0 i64.const 0 struct.set $s 0))",
  "(module (type $a (array i8)) (func (param (ref $a)) local.get 0 i32.const 0 array.get $a drop))",
  "(module (type $a (array (mut i32))) (func (param (ref $a)) local.get 0 i32.const 0 i64.const 0 array.set $a))",
  "(module (type $a (array i32)) (func (param (ref $a)) local.get 0 i32.const 0 i32.const 0 array.set $a))",
  "(module (type $a (array i32)) (func (result (ref $a)) i32.const 0 array.new $a))",
  "(module (type $s (struct)) (type $t (struct)) (func (param (ref $s)) (result (ref $t)) local.get 0))",
  "(module (type $s (sub (struct))) (type $t (sub $s (struct (field i32)))) (func (param (ref $s)) (result (ref $t)) local.get 0))",
  "(module (type $s (sub final (struct))) (type $t (sub $s (struct))))",
  "(module (type $t (sub $t (struct))))",
  "(module (type $t (struct (field (ref 5)))))",
  "(module (rec (type $a (sub (struct))) (type $b (sub $a (struct (field i32))))) (type $c (sub $b (struct))))",
  "(module (type $s (struct)) (func (param anyref) (result (ref $s)) local.get 0))",
  "(module (func (param i31ref) (result eqref) local.get 0) (func (param eqref) (result i31ref) local.get 0))",
  "(module (func (param anyref) (result externref) local.get 0))",
  "(module (func (param anyref) local.get 0 ref.cast (ref null 9) drop))",
  "(module (func (param anyref) (result i32) local.get 0 ref.test i31 drop i64.const 0))",
  "(module (func i32.const 0 ref.i31 i32.const 0 i31.get_s drop))",
  "(module (func (param (ref i31)) local.get 0 i31.get_s drop))",
  "(module (tag (param i32)) (func i64.const 0 throw 0))",
  "(module (func throw 0))",
  "(module (tag) (func (result i32) try (result i32) i32.const 0 catch 0 end))",
  "(module (tag (param i64)) (func try catch 0 drop end))",
  "(module (func rethrow 0))",
  "(module (tag) (func try catch 0 rethrow 1 end))",
  "(module (func (param i32) (result i32) local.get 0 local.get 0 i32.const 1 i32.add i32.add i64.extend_i32_s))",
  "(module (func (result f32) f64.const 0 f32.demote_f64 f32.neg i32.trunc_f32_s))",
  "(module (func (result v128) v128.const i32x4 0 0 0 0 i32.const 0 i32x4.add))",
  "(module (func (result i32) i32.const 0 memory.atomic.notify))",
  "(module (memory 1) (func i32.const 0 i32.const 0 i32.atomic.store align=1))",
  "(module (memory 1) (func i32.const 1 i32.atomic.load drop))",
  "(module (memory 1) (func i32.const 0 i32.const 0 i32.const 0 memory.copy 0 1))",
  "(module (memory 1) (func i32.const 0 i32.const 0 memory.fill 0))",
  "(module (memory 1) (data \"a\") (func i32.const 0 i32.const 0 i32.const 0 memory.init 1))",
  "(module (func data.drop 0))",
  "(module (memory 1) (data $d \"a\") (func data.drop 1))",
  "(module (func elem.drop 0))",
  "(module (func (param i32) (result i32) (block (result i32) local.get 0 br 5)))",
  "(module (func (param i32) (loop local.get 0 br_if 2)))",
  "(module (func (result i32) (if (result i32) (i32.const 1) (then (i32.const 1)))))",
  "(module (func (result i32) (if (result i32) (i32.const 1) (then (i64.const 1)) (else (i32.const 2)))))",
  "(module (func (param i32 i32) (result i32) local.get 0 local.get 1 select (result i64)))",
  "(module (func (param funcref funcref i32) (result funcref) local.get 0 local.get 1 local.get 2 select))",
  "(module (func (param i32) (local i64) local.get 1 local.get 0 i32.add drop))",
  "(module (func (local i32 i32 i32 i32 i32 i32 i32 i32) local.get 8 drop))",
  "(module (func (result i32) (return_call 1)) (func (result i64) i64.const 0))",
  "(module (func (result i32) (return_call_indirect (type 0) (i32.const 0))))",
  "(module (table 1 funcref) (type (func (result i64))) (func (result i32) (return_call_indirect (type 0) (i32.const 0))))",
  "(module (type (func)) (func (call_ref 0)))",
  "(module (type (func)) (func (param (ref null 0)) local.get 0 call_ref 0 i32.const 0))",
  "(module (type (func (param i32))) (func (param (ref null 0)) local.get 0 call_ref 0))",
  "(module (func (param (ref null func)) local.get 0 br_on_null 0 drop))",
  "(module (func (param anyref) (result anyref) block (result (ref any)) local.get 0 br_on_non_null 0 unreachable end))",
  "(module (func (param anyref) block (result i31ref) local.get 0 br_on_cast 0 anyref i31ref end drop))",
  "(module (func (param i32) (result i32) local.get 0 ref.as_non_null))",
];
// Os que o montador do wasm-tools recusa (sintaxe) ficam de fora; os que ele monta mas o validador recusa entram.
const invalidBytes = invalidWats.map(bytesOf).filter((bytes) => bytes !== null);
let invalidIndex = 0;
for (const bytes of invalidBytes) {
  const id = invalidIndex++;
  add(
    `var b = ${bytes}; L(WebAssembly.validate(b)); L(T(() => new WebAssembly.Module(b))); P(WebAssembly.compile(b), 'c${id}')`,
    `var b = ${bytes}; P(WebAssembly.instantiate(b), 'i${id}'); L(T(() => new WebAssembly.Instance(b)))`
  );
}
// Cada módulo válido e cada um dos inválidos, com o byte final (`end`) cortado ou um byte extra no fim.
for (const bytes of invalidBytes.slice(0, 60)) {
  add(`var b = ${bytes}; var c = b.slice(0, b.length - 1); L(WebAssembly.validate(c)); L(T(() => new WebAssembly.Module(c)))`);
  add(`var b = ${bytes}; var c = new Uint8Array(b.length + 1); c.set(b); L(WebAssembly.validate(c)); L(T(() => new WebAssembly.Module(c)))`);
}

// Referência não nula e valor inicial em Table e Global: `externref` omitido vira `undefined`, `anyfunc` vira `null`,
// `(ref $t)` não anulável recusa `null` e exige o valor em set/grow, e os imports de Global por tipo de referência.
{
  const pre = "var f = new WebAssembly.Instance(new WebAssembly.Module(ADD)).exports.add; var g0 = new WebAssembly.Global({ value: 'anyfunc' }, f);";
  const T0 = "sec(1, [1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f])";
  const body = "sec(10, [1, 7, 0, 0x20, 0, 0x20, 1, 0x6a, 0x0b])";
  // Tabela (ref $t) de um elemento com `ref.func 0`, exportada como `tab`, mais a função `f`.
  const refTable = `mk(${T0}, sec(3, [1, 0]), sec(4, [1, 0x40, 0x00, 0x64, 0, 0, 1, 0xd2, 0, 0x0b]), sec(7, [2].concat(str('tab'), [1, 0], str('f'), [0, 0])), ${body})`;
  // Global (ref $t) com `ref.func 0`, imutável (0) ou mutável (1).
  const refGlobal = (mutable) => `mk(${T0}, sec(3, [1, 0]), sec(6, [1, 0x64, 0, ${mutable}, 0xd2, 0, 0x0b]), sec(7, [2].concat(str('g'), [3, 0], str('f'), [0, 0])), ${body})`;
  add(
    "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 2 }, f); L(t.get(0) === f); L(t.get(1) === f)".replace(/^/, pre),
    `${pre}var t = new WebAssembly.Table({ element: 'externref', initial: 2 }, 'x'); L(t.get(1))`,
    "var t = new WebAssembly.Table({ element: 'externref', initial: 2 }); L(R(t.get(1)))",
    "var t = new WebAssembly.Table({ element: 'externref', initial: 2 }, undefined); L(R(t.get(1)))",
    "var t = new WebAssembly.Table({ element: 'externref', initial: 2 }, null); L(R(t.get(1)))",
    "var t = new WebAssembly.Table({ element: 'externref', initial: 2 }, 5); t.set(0); L(R(t.get(0))); L(R(t.get(1)))",
    "var t = new WebAssembly.Table({ element: 'externref', initial: 1 }); L(t.grow(1)); L(R(t.get(1)))",
    "var t = new WebAssembly.Table({ element: 'externref', initial: 1 }); L(t.grow(1, 7)); L(R(t.get(1))); L(t.grow(1, null)); L(R(t.get(2)))",
    `${pre}var t = new WebAssembly.Table({ element: 'anyfunc', initial: 2 }, f); t.set(0); L(R(t.get(0))); L(R(t.get(1)) === 'null')`,
    `${pre}var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1 }); L(t.grow(1, f)); L(t.get(1) === f); L(R(t.get(0)))`,
    "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1 }, 5)",
    "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1 }, function () {})",
    "var g = new WebAssembly.Global({ value: 'externref' }); L(R(g.value))",
    "var g = new WebAssembly.Global({ value: 'externref' }, undefined); L(R(g.value))",
    "var g = new WebAssembly.Global({ value: 'externref', mutable: true }, 'a'); L(g.value); g.value = 'b'; L(g.value); g.value = undefined; L(R(g.value)); g.value = null; L(R(g.value))",
    "var g = new WebAssembly.Global({ value: 'externref' }, 3); L(g.value); L(T(() => { g.value = 4 }))",
    "var g = new WebAssembly.Global({ value: 'anyfunc' }); L(R(g.value))",
    "var g = new WebAssembly.Global({ value: 'anyfunc' }, undefined); L(R(g.value))",
    `${pre}var g = new WebAssembly.Global({ value: 'anyfunc', mutable: true }, f); L(g.value === f); g.value = null; L(R(g.value)); g.value = f; L(g.value === f); L(T(() => { g.value = 1 })); L(T(() => { g.value = undefined })); L(T(() => { g.value = function () {} }))`,
    "var g = new WebAssembly.Global({ value: 'anyfunc' }, 5)",
    "var g = new WebAssembly.Global({ value: 'anyfunc' }, {})",
    "var g = new WebAssembly.Global({ value: 'anyfunc' }, function () {})",
    // Tabela exportada de tipo (ref $t).
    `var b = ${refTable}; L(WebAssembly.validate(b)); var i = new WebAssembly.Instance(new WebAssembly.Module(b)); var t = i.exports.tab; L(t.length); L(t.get(0) === i.exports.f)`,
    `var b = ${refTable}; var i = new WebAssembly.Instance(new WebAssembly.Module(b)); var t = i.exports.tab; L(T(() => t.set(0, null))); L(T(() => t.set(0))); L(T(() => t.set(0, undefined))); L(T(() => t.set(0, 5))); L(T(() => t.set(0, i.exports.f)))`,
    `var b = ${refTable}; var i = new WebAssembly.Instance(new WebAssembly.Module(b)); var t = i.exports.tab; L(T(() => t.grow(1))); L(T(() => t.grow(1, null))); L(T(() => t.grow(1, i.exports.f))); L(t.length); L(t.get(1) === i.exports.f)`,
    `${pre}var b = ${refTable}; var t = new WebAssembly.Instance(new WebAssembly.Module(b)).exports.tab; L(T(() => t.grow(1, f))); L(T(() => t.set(0, f))); L(t.length)`,
    `var b = mk(${T0}, sec(4, [1, 0x64, 0, 0, 1])); L(WebAssembly.validate(b)); L(T(() => new WebAssembly.Module(b)))`,
    // Global exportado de tipo (ref $t).
    `var b = ${refGlobal(0)}; L(WebAssembly.validate(b)); var i = new WebAssembly.Instance(new WebAssembly.Module(b)); L(i.exports.g.value === i.exports.f); L(T(() => { i.exports.g.value = i.exports.f }))`,
    `${pre}var b = ${refGlobal(1)}; var i = new WebAssembly.Instance(new WebAssembly.Module(b)); var g = i.exports.g; L(g.value === i.exports.f); L(T(() => { g.value = null })); L(T(() => { g.value = undefined })); L(T(() => { g.value = 5 })); L(T(() => { g.value = i.exports.f })); L(T(() => { g.value = f }))`,
    // Imports de Global por tipo de referência: (ref null func), (ref func), (ref null extern), (ref extern), (ref $t), (ref null $t).
    ...[["nullfunc", "[0x63, 0x70]"], ["func", "[0x64, 0x70]"], ["nullextern", "[0x63, 0x6f]"], ["extern", "[0x64, 0x6f]"], ["ref0", "[0x64, 0]"], ["nullref0", "[0x63, 0]"], ["funcref", "[0x70]"], ["externref", "[0x6f]"]].flatMap(([, type]) =>
      ["null", "undefined", "5", "f", "TRAPX", "function () {}", "{}", "g0", "new WebAssembly.Global({ value: 'externref' }, 1)", "new WebAssembly.Global({ value: 'i32' }, 1)"].map(
        (value) =>
          `${pre}var TRAPX = new WebAssembly.Instance(new WebAssembly.Module(TRAP)).exports.f; var b = mk(${T0}, sec(2, [1].concat(str('m'), str('g'), [3].concat(${type}, [0])))); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(b), { m: { g: ${value} } })))`
      )
    ),
    // Imports de Table por tipo de elemento.
    ...[["nullfunc", "[0x63, 0x70]"], ["ref0", "[0x64, 0]"], ["funcref", "[0x70]"], ["externref", "[0x6f]"]].flatMap(([, type]) =>
      ["new WebAssembly.Table({ element: 'anyfunc', initial: 1 }, f)", "new WebAssembly.Table({ element: 'anyfunc', initial: 1 })", "new WebAssembly.Table({ element: 'externref', initial: 1 })"].map(
        (value) => `${pre}var b = mk(${T0}, sec(2, [1].concat(str('m'), str('t'), [1].concat(${type}, [0, 1])))); L(T(() => new WebAssembly.Instance(new WebAssembly.Module(b), { m: { t: ${value} } })))`
      )
    )
  );
}

const lines = [];
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
programs.splice(0, programs.length, ...programs.filter((p) => !usesHostApi(p)));
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness +
    `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 50);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
