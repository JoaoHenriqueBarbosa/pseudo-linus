// Gera tests/golden/wasm_js_bun.tsv: programas da ponte JS do WebAssembly (Module, Instance, Memory, Table,
// Global, compile, instantiate, customSections, erros), avaliados no bun. Cada programa registra eventos no
// array global `log` e usa os auxiliares (L, E, D, P, T e os módulos binários ADD, IMP, MEM, TAB, GLB, START,
// TRAP, DIV, CUSTOM...) de tests/golden/wasm_js_bun_harness.js, o mesmo texto que tests/wasm_js_bun_golden.rs
// embute. Colunas: fonte, depois o JSON do log depois de esvaziar as microtarefas, ou
// `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona.
// Os programas não dependem da ordem entre microtarefas e a tarefa em que o JSC assenta uma compilação
// (ver a LACUNA em src/runtime/js_web_assembly.rs): cada um registra só os resultados das suas promessas.
// Cada programa roda num processo bun próprio, com timeout. Uso:
//   bun scripts/gen-wasm-js-golden.js > tests/golden/wasm_js_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness = fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_js_bun_harness.js"), "utf8");
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

// Módulo e instância, chamada simples.
add(
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); L(i.exports.add(2, 3)); L(i.exports.add.length); L(i.exports.add.name); L(Object.isFrozen(i.exports)); L(Object.getPrototypeOf(i.exports) === null)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); L(i.exports.add(2147483647, 1)); L(i.exports.add('7', 1.9)); L(i.exports.add()); L(typeof i.exports.add)",
  "L(WebAssembly.validate(ADD)); L(WebAssembly.validate(EMPTY)); L(WebAssembly.validate(new Uint8Array([1, 2])))",
  "L(JSON.stringify(WebAssembly.Module.exports(new WebAssembly.Module(ADD)))); L(JSON.stringify(WebAssembly.Module.imports(new WebAssembly.Module(IMP))))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: x => x * 2 } }); L(i.exports.g(21))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: () => { throw new RangeError('boom') } } }); L(T(() => i.exports.g(1)))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMP))))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMP), {})))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: {} })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: 1 } })))",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMP), 5)))",
  "L(T(() => new WebAssembly.Module(EMPTY.slice(0, 3))))",
  "L(T(() => new WebAssembly.Module(1)))",
  "L(T(() => WebAssembly.Module(ADD)))",
  "L(T(() => WebAssembly.Instance(new WebAssembly.Module(ADD))))",
  "L(T(() => new WebAssembly.Instance(1)))",
  // Traps.
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TRAP)); L(T(() => i.exports.t()))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(DIV)); L(T(() => i.exports.d()))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TRAP)); try { i.exports.t() } catch (e) { L(e instanceof WebAssembly.RuntimeError); L(e instanceof Error); L(e.name) }",
  // Start, memória, tabela, global exportados.
  "var i = new WebAssembly.Instance(new WebAssembly.Module(START)); L(i.exports.g.value)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(MEM)); L(i.exports.mem instanceof WebAssembly.Memory); L(i.exports.mem.buffer.byteLength); L(i.exports.mem === i.exports.mem); L(i.exports.mem.grow(1)); L(i.exports.mem.buffer.byteLength)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(MEM)); var b = i.exports.mem.buffer; i.exports.mem.grow(1); L(b.byteLength); L(i.exports.mem.buffer === b)",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TAB)); L(i.exports.tab instanceof WebAssembly.Table); L(i.exports.tab.length); L(i.exports.tab.get(0)); L(i.exports.tab.grow(1)); L(i.exports.tab.length); L(T(() => i.exports.tab.get(9)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(GLB)); L(i.exports.g instanceof WebAssembly.Global); L(i.exports.g.value); i.exports.g.value = 5; L(i.exports.g.valueOf())",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2 }); L(m.buffer.byteLength); L(m.grow(1)); L(T(() => m.grow(1))); L(m.buffer.byteLength)",
  "L(T(() => new WebAssembly.Memory()))",
  "L(T(() => new WebAssembly.Memory({})))",
  "L(T(() => new WebAssembly.Memory({ initial: 2, maximum: 1 })))",
  "L(T(() => WebAssembly.Memory({ initial: 1 })))",
  "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 2 }); L(t.length); L(t.get(1)); L(t.set(0, null)); L(T(() => t.set(0, 1))); L(t.grow(2)); L(t.length)",
  "L(T(() => new WebAssembly.Table({ element: 'i32', initial: 1 })))",
  "var t = new WebAssembly.Table({ element: 'externref', initial: 2 }); L(t.get(0)); t.set(0, 'x'); L(t.get(0)); t.set(1, { a: 1 }); L(typeof t.get(1)); L(t.grow(1, 'y')); L(t.get(2))",
  "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1 }); var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); t.set(0, i.exports.add); L(t.get(0) === i.exports.add); L(t.get(0)(1, 2))",
  "var g = new WebAssembly.Global({ value: 'i32', mutable: true }, 7); L(g.value); g.value = 9; L(g.valueOf()); L(T(() => new WebAssembly.Global({ value: 'i32' }, 1).value = 2))",
  "var g = new WebAssembly.Global({ value: 'i64' }, 5n); L(String(g.value)); L(typeof g.value); L(T(() => new WebAssembly.Global({ value: 'i64' }, 1)))",
  "var g = new WebAssembly.Global({ value: 'f32' }, 1.5); L(g.value); var h = new WebAssembly.Global({ value: 'f64' }); L(h.value)",
  "var g = new WebAssembly.Global({ value: 'externref' }, { a: 1 }); L(typeof g.value); var h = new WebAssembly.Global({ value: 'externref' }); L(h.value)",
  "var g = new WebAssembly.Global({ value: 'anyfunc' }); L(g.value); var i = new WebAssembly.Instance(new WebAssembly.Module(ADD)); var h = new WebAssembly.Global({ value: 'anyfunc' }, i.exports.add); L(h.value === i.exports.add)",
  "L(T(() => new WebAssembly.Global({ value: 'v128' })))",
  "L(T(() => new WebAssembly.Global()))",
  "var m = new WebAssembly.Memory({ initial: 1 }); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: m } }); L('ok')",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPMEM), { m: { mem: {} } })))",
  "var t = new WebAssembly.Table({ element: 'anyfunc', initial: 1 }); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: t } }); L('ok')",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPTAB), { m: { tab: 1 } })))",
  "var g = new WebAssembly.Global({ value: 'i32' }, 3); var i = new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: g } }); L('ok')",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: 3 } }); L('ok')",
  "L(T(() => new WebAssembly.Instance(new WebAssembly.Module(IMPGLB), { m: { g: 'x' } })))",
  // customSections.
  "var m = new WebAssembly.Module(CUSTOM); var r = WebAssembly.Module.customSections(m, 'a'); L(r.length); L(r.map(x => x.constructor.name).join()); L(r.map(x => x.byteLength).join()); L(new Uint8Array(r[0])[0])",
  "var m = new WebAssembly.Module(CUSTOM); L(WebAssembly.Module.customSections(m, 'bc').length); L(WebAssembly.Module.customSections(m, 'zz').length); L(WebAssembly.Module.customSections(m, '').length)",
  "var m = new WebAssembly.Module(CUSTOM); L(T(() => WebAssembly.Module.customSections(m))); L(T(() => WebAssembly.Module.customSections(1, 'a'))); L(T(() => WebAssembly.Module.customSections(m, { toString() { throw new RangeError('x') } })))",
  "var m = new WebAssembly.Module(CUSTOM); L(WebAssembly.Module.customSections(m, 'a')[0] === WebAssembly.Module.customSections(m, 'a')[0])",
  // compile e instantiate.
  "P(WebAssembly.compile(ADD), 'c')",
  "P(WebAssembly.compile(ADD.buffer), 'c')",
  "P(WebAssembly.compile(EMPTY), 'c')",
  "P(WebAssembly.compile(new Uint8Array([1, 2])), 'c')",
  "P(WebAssembly.compile(1), 'c'); P(WebAssembly.compile(), 'd'); P(WebAssembly.compile('x'), 'e')",
  "P(WebAssembly.instantiate(ADD), 'i')",
  "WebAssembly.instantiate(ADD).then(r => L(r.instance.exports.add(20, 22)))",
  "WebAssembly.instantiate(ADD).then(r => { L(Object.keys(r).join()); L(r.module instanceof WebAssembly.Module); L(r.instance instanceof WebAssembly.Instance); L(Object.getPrototypeOf(r) === Object.prototype) })",
  "P(WebAssembly.instantiate(new WebAssembly.Module(ADD)), 'i')",
  "WebAssembly.instantiate(new WebAssembly.Module(ADD)).then(i => L(i.exports.add(1, 2)))",
  "P(WebAssembly.instantiate(new Uint8Array([1, 2])), 'i')",
  "P(WebAssembly.instantiate(1), 'i'); P(WebAssembly.instantiate(), 'j')",
  "P(WebAssembly.instantiate(ADD, 5), 'i'); P(WebAssembly.instantiate(new WebAssembly.Module(ADD), 'x'), 'j')",
  "P(WebAssembly.instantiate(IMP), 'i')",
  "P(WebAssembly.instantiate(IMP, {}), 'i')",
  "P(WebAssembly.instantiate(IMP, { m: { f: 1 } }), 'i')",
  "WebAssembly.instantiate(IMP, { m: { f: x => x + 1 } }).then(r => L(r.instance.exports.g(41)))",
  "P(WebAssembly.instantiate(new WebAssembly.Module(IMP), { m: { f: 1 } }), 'i')",
  "P(WebAssembly.instantiate(new WebAssembly.Module(IMP)), 'i')",
  "WebAssembly.instantiate(START).then(r => L(r.instance.exports.g.value))",
  "WebAssembly.instantiate(TRAP).then(r => L(T(() => r.instance.exports.t())))",
  "P(WebAssembly.instantiate(MEM), 'i'); P(WebAssembly.instantiate(TAB), 'j'); P(WebAssembly.instantiate(GLB), 'k')",
  "var p = WebAssembly.compile(ADD); L(p instanceof Promise); L(Object.getPrototypeOf(p) === Promise.prototype); var q = WebAssembly.instantiate(ADD); L(q instanceof Promise)",
  "var p = WebAssembly.compile(1); L(p instanceof Promise); p.catch(e => L(E(e)))",
  "WebAssembly.compile(ADD).then(m => WebAssembly.instantiate(m)).then(i => L(i.exports.add(3, 4)))",
  "WebAssembly.compile(CUSTOM).then(m => L(WebAssembly.Module.customSections(m, 'a').length))",
  "WebAssembly.compile(EMPTY).then(m => L(JSON.stringify(WebAssembly.Module.exports(m))))",
  // Forma dos objetos: nomes, ordem e descritores medidos no bun.
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Memory.prototype).filter(n => ['grow', 'buffer', 'constructor'].includes(n))))",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Table.prototype)))",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Global.prototype)))",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Instance.prototype)))",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Module.prototype)))",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Module)))",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly).filter(n => ['CompileError', 'Global', 'Instance', 'LinkError', 'Memory', 'Module', 'RuntimeError', 'Table', 'compile', 'instantiate', 'validate', 'compileStreaming', 'instantiateStreaming'].includes(n))))",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'validate'))); L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'compileStreaming'))); L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'Memory')))",
  "L(WebAssembly.compileStreaming.length); L(WebAssembly.compileStreaming.name); L(WebAssembly.instantiateStreaming.length); L(WebAssembly.instantiateStreaming.name)",
  "P(WebAssembly.compileStreaming(1), 'c')",
  "P(WebAssembly.compileStreaming(), 'c')",
  "P(WebAssembly.instantiateStreaming(null), 'c')",
  "P(WebAssembly.instantiateStreaming('x'), 'c')",
  "P(WebAssembly.instantiateStreaming(true), 'c')",
  "P(WebAssembly.instantiateStreaming({}), 'c')",
  "P(WebAssembly.compileStreaming(ADD), 'c')",
  "L(typeof WebAssembly.Memory.prototype.type); L(typeof WebAssembly.Table.prototype.type); L(typeof WebAssembly.Global.prototype.type)",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'compile'))); L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'instantiate')))",
  "L(WebAssembly.compile.length); L(WebAssembly.compile.name); L(WebAssembly.instantiate.length); L(WebAssembly.instantiate.name); L(WebAssembly.Module.customSections.length); L(WebAssembly.Module.customSections.name)",
  "var d = Object.getOwnPropertyDescriptor(WebAssembly.Memory.prototype, 'buffer'); L(typeof d.get); L(typeof d.set); L(d.get.name); L(d.enumerable); L(d.configurable)",
  "var d = Object.getOwnPropertyDescriptor(WebAssembly.Table.prototype, 'length'); L(typeof d.get); L(typeof d.set); L(d.get.name); L(d.enumerable); L(d.configurable)",
  "var d = Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype, 'value'); L(typeof d.get); L(typeof d.set); L(d.get.name); L(d.set.name); L(d.enumerable); L(d.configurable)",
  "var d = Object.getOwnPropertyDescriptor(WebAssembly.Instance.prototype, 'exports'); L(typeof d.get); L(typeof d.set); L(d.get.name)",
  "L(String(WebAssembly)); L(Object.prototype.toString.call(new WebAssembly.Memory({ initial: 0 }))); L(Object.prototype.toString.call(new WebAssembly.Table({ element: 'anyfunc', initial: 0 }))); L(Object.prototype.toString.call(new WebAssembly.Module(ADD))); L(Object.prototype.toString.call(new WebAssembly.Instance(new WebAssembly.Module(ADD))))",
  "L(WebAssembly.Memory.length); L(WebAssembly.Table.length); L(WebAssembly.Global.length); L(WebAssembly.Module.length); L(WebAssembly.Instance.length)",
  "L(new WebAssembly.LinkError('x') instanceof Error); L(new WebAssembly.CompileError('x').name); L(Object.getPrototypeOf(WebAssembly.RuntimeError) === Error)",
  // Tag e Exception.
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly).filter(n => ['CompileError', 'Exception', 'Global', 'Instance', 'LinkError', 'Memory', 'Module', 'RuntimeError', 'Table', 'Tag', 'compile', 'instantiate', 'validate'].includes(n))))",
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'Tag'))); L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'Exception')))",
  "L(WebAssembly.Tag.length); L(WebAssembly.Exception.length); L(WebAssembly.Tag.name); L(WebAssembly.Exception.name)",
  "L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Tag.prototype))); L(JSON.stringify(Object.getOwnPropertyNames(WebAssembly.Exception.prototype)))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); L(Object.prototype.toString.call(t)); L(Object.prototype.toString.call(new WebAssembly.Exception(t, [1, 2]))); L(t.constructor === WebAssembly.Tag); L(new WebAssembly.Exception(t, [1, 2]) instanceof Error)",
  "L(T(() => new WebAssembly.Tag()))",
  "L(T(() => new WebAssembly.Tag(1)))",
  "L(T(() => new WebAssembly.Tag({})))",
  "L(T(() => new WebAssembly.Tag({ parameters: 1 })))",
  "L(T(() => new WebAssembly.Tag({ parameters: ['x'] })))",
  "L(T(() => new WebAssembly.Tag({ parameters: [1] })))",
  "L(T(() => WebAssembly.Tag({ parameters: [] })))",
  "L(Object.prototype.toString.call(new WebAssembly.Tag({ parameters: ['anyfunc', 'externref', 'v128'] }))); L(Object.prototype.toString.call(new WebAssembly.Tag({ parameters: new Set(['i32']) })))",
  "L(T(() => new WebAssembly.Exception()))",
  "L(T(() => new WebAssembly.Exception({})))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); L(T(() => new WebAssembly.Exception(t, [1])))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); L(T(() => new WebAssembly.Exception(t, [1, 2], 1)))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); L(T(() => WebAssembly.Exception(t, [])))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); var e = new WebAssembly.Exception(t, [1.5, 2.5]); L(e.getArg(t, 0)); L(e.getArg(t, 1)); L(e.getArg(t, '1')); L(e.is(t)); L(e.is(new WebAssembly.Tag({ parameters: ['i32', 'f64'] })))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); var e = new WebAssembly.Exception(t, [1, 2]); L(T(() => e.getArg(t, 2))); L(T(() => e.getArg(t, -1))); L(T(() => e.getArg(t, 2 ** 32))); L(T(() => e.getArg(t)))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); var e = new WebAssembly.Exception(t, [1, 2]); L(T(() => e.getArg({}, 0))); L(T(() => e.getArg(new WebAssembly.Tag({ parameters: ['i32', 'f64'] }), 0))); L(T(() => e.is({})))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); L(T(() => WebAssembly.Exception.prototype.is.call({}, t))); L(T(() => WebAssembly.Exception.prototype.getArg.call({}, t, 0)))",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'f64'] }); var e = new WebAssembly.Exception(t, [1, 2]); L(e.stack); L(new WebAssembly.Exception(t, [1, 2], null).stack); L(new WebAssembly.Exception(t, [1, 2], {}).stack); L(T(() => Object.getOwnPropertyDescriptor(WebAssembly.Exception.prototype, 'stack').get.call({})))",
  "var d = Object.getOwnPropertyDescriptor(WebAssembly.Exception.prototype, 'stack'); L(typeof d.get); L(typeof d.set); L(d.get.name); L(d.enumerable); L(d.configurable); var m = Object.getOwnPropertyDescriptor(WebAssembly.Exception.prototype, 'is'); L(m.writable); L(m.enumerable); L(m.configurable); L(WebAssembly.Exception.prototype.getArg.length); L(WebAssembly.Exception.prototype.is.length)",
  "var t = new WebAssembly.Tag({ parameters: ['i32', 'i64', 'externref', 'f32'] }); var o = {}; var e = new WebAssembly.Exception(t, [1.9, 2n, o, 3.5]); L(e.getArg(t, 0)); L(String(e.getArg(t, 1))); L(e.getArg(t, 2) === o); L(e.getArg(t, 3)); L(T(() => new WebAssembly.Exception(t, [1, 2, o, 3])))",
  "var t = new WebAssembly.Tag({ parameters: ['i32'] }); L(T(() => new WebAssembly.Exception(t, [Symbol()])))"
);

// Memory.prototype.toFixedLengthBuffer / toResizableBuffer (bun 1.4.2; sem `type()` em Memory, Table, Global).
add(
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var f = m.toFixedLengthBuffer(); L(f === m.buffer); L(f.byteLength); L(f.resizable); L(f.detached); L(f === m.toFixedLengthBuffer()); L(Object.getOwnPropertyNames(WebAssembly.Memory.prototype).join()); L(WebAssembly.Memory.prototype.toFixedLengthBuffer.length); L(WebAssembly.Memory.prototype.toResizableBuffer.length)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(r.resizable); L(r.byteLength); L(r.maxByteLength); L(r.detached); L(r === m.toResizableBuffer()); L(r === m.buffer); L(m.buffer.resizable); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var f = m.toFixedLengthBuffer(); var r = m.toResizableBuffer(); L(f.detached); L(f.byteLength); L(r.detached); L(r.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var f = m.toFixedLengthBuffer(); var r = m.toResizableBuffer(); m.grow(1); L(f.detached); L(r.detached); L(r.byteLength); L(m.buffer.byteLength); L(r === m.toResizableBuffer()); L(m.toFixedLengthBuffer() === f)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var f = m.toFixedLengthBuffer(); m.grow(1); L(f.detached); L(f.byteLength); L(m.toFixedLengthBuffer() === f); L(m.toFixedLengthBuffer().byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1 }); var r = m.toResizableBuffer(); L(r.resizable); L(r.maxByteLength); L(r.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); new Uint8Array(r)[5] = 7; L(new Uint8Array(m.buffer)[5]); m.grow(1); L(new Uint8Array(r)[5]); L(new Uint8Array(m.buffer)[5])",
  "L(T(() => WebAssembly.Memory.prototype.toFixedLengthBuffer.call({}))); L(T(() => WebAssembly.Memory.prototype.toResizableBuffer.call({})))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var f = m.toFixedLengthBuffer(); var r = m.toResizableBuffer(); var g = m.toFixedLengthBuffer(); L(g === f); L(g === r); L(g.resizable); L(g.detached); L(f.detached); L(r.detached); L(m.buffer === g); L(r === m.toResizableBuffer())",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); var g = m.toFixedLengthBuffer(); L(g === r); L(g.resizable); L(r.detached); L(m.buffer === g)"
);

// ArrayBuffer redimensionável de Memory.toResizableBuffer: resize cresce a memória, transfer lança (bun 1.4.2).
add(
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(r.resize(131072)); L(r.byteLength); L(m.buffer.byteLength); L(m.buffer === r); L(r.detached)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(T(() => r.resize(70000))); L(T(() => r.resize(262144))); L(r.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 2, maximum: 3 }); var r = m.toResizableBuffer(); L(T(() => r.resize(65536))); L(r.byteLength); L(T(() => r.resize(131072))); L(r.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(T(() => r.resize(196608))); L(r.byteLength); L(T(() => m.grow(1))); L(m.buffer.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); var u = new Uint8Array(r); u[1] = 5; r.resize(131072); L(u.length); L(u[1]); L(new Uint8Array(m.buffer).length); L(m.grow(1)); L(r.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(T(() => r.transfer())); L(T(() => r.transferToFixedLength())); L(r.detached); L(r.byteLength)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var f = m.buffer; L(T(() => f.transfer())); L(T(() => f.transferToFixedLength())); L(f.detached)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(T(() => r.transfer(10))); L(T(() => r.slice(0, 4).byteLength))",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 3 }); var r = m.toResizableBuffer(); L(T(() => r.resize())); L(r.byteLength); L(T(() => r.resize(-1))); L(T(() => r.resize(Infinity)))",
  "var m = new WebAssembly.Memory({ initial: 1 }); var r = m.toResizableBuffer(); L(T(() => r.resize(131072))); L(r.byteLength); L(m.buffer === r)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); L(Object.prototype.toString.call(m.buffer)); L(m.buffer.byteLength); L(m.buffer.growable); L(m.buffer.maxByteLength); L(m.buffer === m.buffer)",
  "L(T(() => new WebAssembly.Memory({ initial: 1, shared: true })))",
  "L(T(() => new WebAssembly.Memory({ initial: 3, maximum: 2, shared: true })))",
  "L(new WebAssembly.Memory({ initial: 1, maximum: 2, shared: 1 }).buffer.constructor.name); L(new WebAssembly.Memory({ initial: 1, maximum: 2, shared: false }).buffer.constructor.name)",
  "var m = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }); L(m.grow(1)); L(m.buffer.byteLength); L(T(() => m.grow(1))); L(Object.prototype.toString.call(m.buffer))"
);

// Atomics (prefixo 0xFE) no interpretador: AT(sub, params, results, imm, shared) do harness; 0x7f é i32, 0x7e é i64.
add(
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(0, [0x7f, 0x7f], [0x7f], [2, 0], true))).exports.f; L(f(0, 1)); L(T(() => f(1, 1))); L(T(() => f(65536, 1)))",
  "L(new WebAssembly.Instance(new WebAssembly.Module(AT(0, [0x7f, 0x7f], [0x7f], [2, 0], false))).exports.f(0, 1))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(1, [0x7f, 0x7f, 0x7e], [0x7f], [2, 0], true))).exports.f; L(f(0, 0, 0n)); L(f(0, 5, 0n)); L(T(() => f(2, 0, 0n))); L(T(() => f(65536, 0, 0n)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(1, [0x7f, 0x7f, 0x7e], [0x7f], [2, 0], false))).exports.f; L(T(() => f(0, 0, 0n)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(2, [0x7f, 0x7e, 0x7e], [0x7f], [3, 0], true))).exports.f; L(f(0, 0n, 0n)); L(f(0, 5n, 0n)); L(T(() => f(4, 0n, 0n)))",
  "L(new WebAssembly.Instance(new WebAssembly.Module(AT(3, [], [], [0], true))).exports.f())",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(16, [0x7f], [0x7f], [2, 0], true))).exports.f; L(f(0)); L(T(() => f(2))); L(T(() => f(65536))); L(T(() => f(65533)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(17, [0x7f], [0x7e], [3, 0], true))).exports.f; L(String(f(8))); L(T(() => f(4))); L(T(() => f(65536)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(18, [0x7f], [0x7f], [0, 0], true))).exports.f; L(f(65535)); L(T(() => f(65536)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(19, [0x7f], [0x7f], [1, 0], false))).exports.f; L(f(2)); L(T(() => f(1)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(22, [0x7f], [0x7e], [2, 0], true))).exports.f; L(String(f(4))); L(T(() => f(1)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(23, [0x7f, 0x7f], [], [2, 0], true))).exports.f; L(f(4, 7)); L(T(() => f(3, 7))); L(T(() => f(65536, 7)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(28, [0x7f, 0x7e], [], [1, 0], true))).exports.f; L(f(2, 9n)); L(T(() => f(1, 9n))); L(T(() => f(65536, 9n)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(30, [0x7f, 0x7f], [0x7f], [2, 0], true))).exports.f; L(f(0, 5)); L(f(0, 6)); L(f(0, -1)); L(T(() => f(1, 1))); L(T(() => f(65536, 1)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(39, [0x7f, 0x7f], [0x7f], [0, 0], true))).exports.f; L(f(0, 1)); L(f(0, 2)); L(f(1, 3))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(44, [0x7f, 0x7f], [0x7f], [2, 0], true))).exports.f; L(f(0, 255)); L(f(0, 255))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(51, [0x7f, 0x7f], [0x7f], [2, 0], true))).exports.f; L(f(0, 5)); L(f(0, 2)); L(f(0, 0))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(66, [0x7f, 0x7e], [0x7e], [3, 0], true))).exports.f; L(String(f(0, 5n))); L(String(f(0, 7n))); L(T(() => f(4, 1n))); L(T(() => f(65536, 1n)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(72, [0x7f, 0x7f, 0x7f], [0x7f], [2, 0], true))).exports.f; L(f(0, 0, 9)); L(f(0, 0, 3)); L(f(0, 9, 3)); L(T(() => f(1, 0, 1))); L(T(() => f(65536, 0, 1)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(AT(73, [0x7f, 0x7e, 0x7e], [0x7e], [3, 0], true))).exports.f; L(String(f(0, 0n, 5n))); L(String(f(0, 5n, 6n))); L(String(f(0, 5n, 7n))); L(T(() => f(4, 0n, 1n)))"
);

// Extensões do núcleo (FN(params, results, corpo) do harness; 0x7f i32, 0x7e i64, 0x7d f32, 0x7c f64, 0x7b v128):
// conversões saturantes (0xFC 0-7), extensão de sinal, bulk memory, multi-value, tail call, tipos de referência,
// memory.size/grow, e `WebAssembly.validate` de SIMD, GC, memory64, multi-memory e extended-const.
const V128C = "[0xfd, 12, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]";
add(
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7d], [0x7f], [0x20, 0, 0xfc, 0]))).exports.f; L(f(NaN)); L(f(1e10)); L(f(-1e10)); L(f(3.7)); L(f(-3.7))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7d], [0x7f], [0x20, 0, 0xfc, 1]))).exports.f; L(f(NaN)); L(f(1e10)); L(f(-5)); L(f(3.7))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7c], [0x7f], [0x20, 0, 0xfc, 2]))).exports.f; L(f(1e300)); L(f(-1e300)); L(f(2147483647.9))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7c], [0x7f], [0x20, 0, 0xfc, 3]))).exports.f; L(f(4294967296)); L(f(-1)); L(f(4294967295.5))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7c], [0x7e], [0x20, 0, 0xfc, 6]))).exports.f; L(String(f(1e30))); L(String(f(-1e30))); L(String(f(NaN))); L(String(f(123456789012.5)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7c], [0x7e], [0x20, 0, 0xfc, 7]))).exports.f; L(String(f(1e30))); L(String(f(-1))); L(String(f(18446744073709549568)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f], [0x7f], [0x20, 0, 0xc0]))).exports.f; L(f(255)); L(f(128)); L(f(127)); L(f(0x1ff))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f], [0x7f], [0x20, 0, 0xc1]))).exports.f; L(f(0xffff)); L(f(0x8000)); L(f(0x7fff))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7e], [0x7e], [0x20, 0, 0xc4]))).exports.f; L(String(f(0xffffffffn))); L(String(f(0x80000000n))); L(String(f(0x7fffffffn)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([], [0x7f], [0x41, 0, 0x41, 7, 0x41, 4, 0xfc, 11, 0, 0x41, 0, 0x28, 2, 0]))).exports.f; L(f())",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f], [], [0x20, 0, 0x41, 1, 0x41, 2, 0xfc, 11, 0]))).exports.f; L(f(65534)); L(T(() => f(65535))); L(T(() => f(65536)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([], [0x7f], [0x41, 0, 0x41, 9, 0x41, 4, 0xfc, 11, 0, 0x41, 4, 0x41, 0, 0x41, 4, 0xfc, 10, 0, 0, 0x41, 4, 0x28, 2, 0]))).exports.f; L(f())",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f], [], [0x41, 0, 0x20, 0, 0x41, 8, 0xfc, 10, 0, 0]))).exports.f; L(f(0)); L(T(() => f(65529))); L(T(() => f(65528)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f], [0x7f, 0x7f], [0x20, 0, 0x20, 0, 0x41, 1, 0x6a]))).exports.f; L(JSON.stringify(f(41)))",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f, 0x7f], [0x7f], [0x20, 0, 0x45, 0x04, 0x7f, 0x20, 1, 0x05, 0x20, 0, 0x41, 1, 0x6b, 0x20, 1, 0x41, 2, 0x6a, 0x12, 0, 0x0b]))).exports.f; L(f(10, 0)); L(f(1000000, 0))",
  "L(new WebAssembly.Instance(new WebAssembly.Module(FN([], [0x7f], [0xd0, 0x70, 0xd1]))).exports.f())",
  "L(new WebAssembly.Instance(new WebAssembly.Module(FN([], [0x7f], [0x3f, 0]))).exports.f())",
  "var f = new WebAssembly.Instance(new WebAssembly.Module(FN([0x7f], [0x7f], [0x20, 0, 0x40, 0]))).exports.f; L(f(1)); L(f(0)); L(f(65536))",
  "L(WebAssembly.validate(FN([], [0x7b], " + V128C + "))); L(WebAssembly.validate(FN([], [0x7b], " + V128C + ".concat(" + V128C + ", [0xfd, 174, 1])))); L(WebAssembly.validate(FN([], [0x7f], " + V128C + ".concat([0xfd, 27, 0]))))",
  "L(WebAssembly.validate(mk(sec(5, [1, 4, 1])))); L(WebAssembly.validate(mk(sec(5, [2, 0, 1, 0, 1])))); L(WebAssembly.validate(mk(sec(6, [1, 0x7f, 0, 0x41, 1, 0x41, 2, 0x6a, 0x0b]))))"
);

// SIMD executado (prefixo 0xFD): o `v128` não cruza a fronteira com o JS, então cada programa extrai lanes, mascaras ou
// escalares no fim. `vc` empilha v128.const, `vx` codifica o subopcode LEB, `v32`/`v16` empacotam lanes,
// `vrun` monta um módulo de uma função "f" (sem parâmetros; `locals` opcional) com memória de uma página.
const SIMD_PRELUDE =
  "var vc=function(a){return [0xfd,12].concat(a)};" +
  "var vx=function(o){return o<128?[0xfd,o]:[0xfd,(o&127)|128,o>>7]};" +
  "var v32=function(a){var r=[];a.forEach(function(v){r.push(v&255,(v>>8)&255,(v>>16)&255,(v>>24)&255)});return r};" +
  "var v16=function(a){var r=[];a.forEach(function(v){r.push(v&255,(v>>8)&255)});return r};" +
  "var f4=function(x){return [0x43].concat(Array.from(new Uint8Array(new Float32Array([x]).buffer)))};" +
  "var vrun=function(r,b,l){var c=(l||[0]).concat(b,[0x0b]);return new WebAssembly.Instance(new WebAssembly.Module(mk(sec(1,[1,0x60,0,r.length].concat(r)),sec(3,[1,0]),sec(5,[1,0,1]),sec(7,[1].concat(str(\"f\"),[0,0])),sec(10,[1,c.length].concat(c)))))};";
const simdRun = (results, body, locals) =>
  SIMD_PRELUDE + "L(vrun(" + results + ", " + body + (locals ? ", " + locals : "") + ").exports.f())";
const simdRun64 = (results, body) =>
  SIMD_PRELUDE + "L(String(vrun(" + results + ", " + body + ").exports.f()))";
const I = "[0x7f]";
const ramp = "[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]";
const ramp2 = "[16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]";
programs.push(
  simdRun(I, "vc(v32([1,2,3,4])).concat([0x41,10], vx(0x11), vx(0xae), vx(0x1b), [2])"),
  simdRun(I, "vc(v32([1,2,3,4])).concat([0x41,10], vx(0x11), vx(0xb1), vx(0x1b), [1])"),
  simdRun(I, "vc(v32([1,2,3,4])).concat([0x41,10], vx(0x11), vx(0xb5), vx(0x1b), [3])"),
  simdRun(I, "vc(Array(16).fill(255)).concat(vx(0x15), [0])") + "; " + "L(vrun(" + I + ", vc(Array(16).fill(255)).concat(vx(0x16), [0])).exports.f())",
  simdRun("[0x7c]", "[0x44,0,0,0,0,0,0,0xf8,0x3f].concat(vx(0x14), [0x44,0,0,0,0,0,0,0xf8,0x3f], vx(0x14), vx(0xf0), vx(0x21), [1])"),
  simdRun(I, "vc(v32([-16,2,3,4])).concat([0x41,2], vx(0xac), vx(0x1b), [0])") + "; L(vrun(" + I + ", vc(v32([-16,2,3,4])).concat([0x41,2], vx(0xad), vx(0x1b), [0])).exports.f()); L(vrun(" + I + ", vc(v32([-16,2,3,4])).concat([0x41,33], vx(0xab), vx(0x1b), [1])).exports.f())",
  simdRun(I, "vc(v32([-5,2,3,4])).concat(vx(0xa0), vx(0x1b), [0])") + "; L(vrun(" + I + ", vc(v32([-2147483648,2,3,4])).concat(vx(0xa1), vx(0x1b), [0])).exports.f())",
  simdRun(I, "vc(v16(Array(8).fill(65535))).concat(vc(v16(Array(8).fill(1))), vx(0x96), vx(0x18), [0])") +
    "; L(vrun(" + I + ", vc(v16(Array(8).fill(65535))).concat(vc(v16(Array(8).fill(1))), vx(0x97), vx(0x19), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(v16(Array(8).fill(65535))).concat(vc(v16(Array(8).fill(1))), vx(0x99), vx(0x19), [0])).exports.f())",
  simdRun(I, "vc(v16(Array(8).fill(300))).concat(vc(v16(Array(8).fill(-300))), vx(0x65), vx(0x15), [0])") +
    "; L(vrun(" + I + ", vc(v16(Array(8).fill(300))).concat(vc(v16(Array(8).fill(-300))), vx(0x65), vx(0x15), [8])).exports.f())" +
    "; L(vrun(" + I + ", vc(v16(Array(8).fill(300))).concat(vc(v16(Array(8).fill(-300))), vx(0x66), vx(0x16), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(v16(Array(8).fill(300))).concat(vc(v16(Array(8).fill(-300))), vx(0x66), vx(0x16), [8])).exports.f())",
  simdRun(I, "vc(Array(16).fill(255)).concat(vx(0x87), vx(0x18), [0])") + "; L(vrun(" + I + ", vc(Array(16).fill(255)).concat(vx(0x89), vx(0x19), [0])).exports.f())",
  simdRun(I, "vc(v16([1,2,3,4,5,6,7,8])).concat(vc(v16([1,2,3,4,5,6,7,8])), vx(0xba), vx(0x1b), [3])") +
    "; L(vrun(" + I + ", vc(v16([1,2,3,4,5,6,7,8])).concat(vc(v16([1,2,3,4,5,6,7,8])), vx(0xba), vx(0x1b), [0])).exports.f())",
  simdRun("[0x7d]", "vc(v32([-5,0,0,0])).concat(vx(0xfa), vx(0x1f), [0])") +
    "; L(vrun(" + I + ", f4(1e10).concat(vx(0x13), vx(0xf8), vx(0x1b), [0])).exports.f())" +
    "; L(vrun(" + I + ", f4(1e10).concat(vx(0x13), vx(0xf9), vx(0x1b), [0])).exports.f())" +
    "; L(vrun(" + I + ", f4(NaN).concat(vx(0x13), vx(0xf8), vx(0x1b), [0])).exports.f())",
  simdRun(I, "vc(Array(16).fill(0x80)).concat(vx(0x64))") +
    "; L(vrun(" + I + ", vc(Array(16).fill(0x80)).concat(vx(0x63))).exports.f())" +
    "; L(vrun(" + I + ", vc(Array(16).fill(0)).concat(vx(0x53))).exports.f())" +
    "; L(vrun(" + I + ", vc(v32([1,2,3,4])).concat(vc(v32([1,0,3,0])), vx(0x37), vx(0x1b), [1])).exports.f())",
  simdRun(I, "vc(Array(16).fill(0xf0)).concat(vc(Array(16).fill(0x3c)), vx(0x4e), vx(0x16), [3])") +
    "; L(vrun(" + I + ", vc(Array(16).fill(0xf0)).concat(vc(Array(16).fill(0x3c)), vx(0x51), vx(0x16), [3])).exports.f())" +
    "; L(vrun(" + I + ", vc(Array(16).fill(0xff)).concat(vc(Array(16).fill(0)), vc(Array(16).fill(0x0f)), vx(0x52), vx(0x16), [3])).exports.f())",
  simdRun(I, "vc(" + ramp + ").concat(vc(" + ramp2 + "), [0xfd,0x0d,31,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14], vx(0x16), [0])") +
    "; L(vrun(" + I + ", vc(" + ramp + ").concat(vc([15,16,0,0,0,0,0,0,0,0,0,0,0,0,0,0]), vx(0x0e), vx(0x16), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(" + ramp + ").concat(vc([15,16,0,0,0,0,0,0,0,0,0,0,0,0,0,0]), vx(0x0e), vx(0x16), [1])).exports.f())",
  simdRun(I, "vc(v32([7,8,9,10])).concat([0x1a], vc(v32([7,8,9,10])), [0x21,0,0x20,0,0x20,0], vx(0xae), vx(0x1b), [2])", "[1,1,0x7b]") +
    "; L(vrun(" + I + ", vc(v32([7,8,9,10])).concat([0x22,0,0x1a,0x20,0], vx(0x1b), [1]), [1,1,0x7b]).exports.f())",
  simdRun(I, "[0x02,0x7b].concat(vc(v32([1,2,3,4])), [0x0c,0,0x0b], vx(0x1b), [1])") +
    "; L(vrun(" + I + ", vc(v32([1,2,3,4])).concat(vc(v32([5,6,7,8])), [0x41,0,0x1c,1,0x7b], vx(0x1b), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(v32([1,2,3,4])).concat(vc(v32([5,6,7,8])), [0x41,1,0x1c,1,0x7b], vx(0x1b), [0])).exports.f())",
  simdRun(I, "[0x41,0].concat(vc(v32([1,2,3,4])), [0xfd,0x0b,4,0, 0x41,0, 0xfd,0,4,0], vx(0x1b), [3])") +
    "; L(vrun(" + I + ", [0x41,0].concat(vc(v32([1,2,3,4])), [0xfd,0x0b,4,0, 0x41,0, 0xfd,2,3,0], vx(0x19), [4])).exports.f())" +
    "; L(vrun(" + I + ", [0x41,0].concat(vc(v32([1,2,3,4])), [0xfd,0x0b,4,0, 0x41,0, 0xfd,9,2,0], vx(0x1b), [2])).exports.f())" +
    "; L(T(function(){return vrun(" + I + ", [0x41,0xf8,0xff,0x03, 0xfd,0,4,0].concat(vx(0x1b), [0])).exports.f()}))" +
    "; L(T(function(){return vrun(" + I + ", [0x41,0xf8,0xff,0x03].concat(vc(v32([1,2,3,4])), [0xfd,0x0b,4,0, 0x41,0])).exports.f()}))",
  simdRun(I, "vc(Array(16).fill(200)).concat(vc(Array(16).fill(100)), vx(0x70), vx(0x16), [0])") +
    "; L(vrun(" + I + ", vc(Array(16).fill(156)).concat(vc(Array(16).fill(100)), vx(0x72), vx(0x15), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(Array(16).fill(255)).concat(vx(0x62), vx(0x16), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(Array(16).fill(1)).concat(vc(Array(16).fill(2)), vx(0x7b), vx(0x16), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(v16(Array(8).fill(-32768))).concat(vc(v16(Array(8).fill(-32768))), vx(0x82), vx(0x18), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(Array(16).fill(255)).concat(vx(0x7c), vx(0x18), [0])).exports.f())" +
    "; L(vrun(" + I + ", vc(Array(16).fill(255)).concat(vc(Array(16).fill(255)), vx(0x9e), vx(0x19), [0])).exports.f())",
  simdRun("[0x7d]", "f4(16).concat(vx(0x13), vx(0xe3), vx(0x1f), [0])") +
    "; L(vrun([0x7d], f4(3).concat(vx(0x13), vx(0xe1), vx(0x1f), [0])).exports.f())" +
    "; L(vrun([0x7d], f4(2.5).concat(vx(0x13), vx(0x6a), vx(0x1f), [0])).exports.f())" +
    "; L(vrun([0x7d], f4(1.2).concat(vx(0x13), vx(0x67), vx(0x1f), [0])).exports.f())" +
    "; L(vrun([0x7d], f4(-1).concat(vx(0x13), f4(2), vx(0x13), vx(0xe7), vx(0x1f), [0])).exports.f())" +
    "; L(vrun([0x7c], f4(1.5).concat(vx(0x13), vx(0x5f), vx(0x21), [0])).exports.f())",
  simdRun64("[0x7e]", "[0x42,5].concat(vx(0x12), [0x42,7], vx(0x12), vx(0xd5), vx(0x1d), [1])") +
    "; L(String(vrun([0x7e], [0x42,5].concat(vx(0x12), [0x42,7], vx(0x12), vx(0xd8), vx(0x1d), [0])).exports.f()))"
);

// Pilha de tipos do validador ligada ao interpretador: `drop` e `select` sem tipo de `v128` depois de `end`/`if`
// (a largura vem da pilha de tipos, não da instrução anterior), e a fronteira com o host: importação com `v128`
// instancia, mas chamá-la lança TypeError antes de o host rodar (WasmToJS do JSC). `vi(p, r, b)` monta um módulo
// com a importação m.f de tipo (p) -> (r) e o exportado g de tipo () -> (i32) com corpo `b`; o host registra "host".
const VI_PRELUDE =
  SIMD_PRELUDE +
  "var vi=function(p,r,b){var c=[0].concat(b,[0x0b]);return new WebAssembly.Instance(new WebAssembly.Module(mk(sec(1,[2,0x60,p.length].concat(p,[r.length],r,[0x60,0,1,0x7f])),sec(2,[1].concat(str(\"m\"),str(\"f\"),[0,0])),sec(3,[1,1]),sec(7,[1].concat(str(\"g\"),[0,1])),sec(10,[1,c.length].concat(c)))),{m:{f:function(){L(\"host\");return 1}}})};";
const vsum = "vc(v32([1,2,3,4]))";
programs.push(
  simdRun(I, "[0x02,0x7b].concat(" + vsum + ", [0x0c,0,0x0b,0x1a,0x41,7])"),
  simdRun(I, "[0x41,1,0x04,0x7b].concat(" + vsum + ", [0x05], " + vsum + ", [0x0b,0x1a,0x41,9])") +
    "; L(vrun(" + I + ", [0x41,0,0x04,0x7b].concat(" + vsum + ", [0x05], " + vsum + ", [0x0b,0x1a,0x41,9])).exports.f())",
  simdRun(I, "[0x03,0x7b].concat(" + vsum + ", [0x0b,0x1a,0x41,5])"),
  simdRun(I, vsum + ".concat(vc(v32([5,6,7,8])), [0x41,1,0x1b], vx(0x1b), [0])") +
    "; L(vrun(" + I + ", " + vsum + ".concat(vc(v32([5,6,7,8])), [0x41,0,0x1b], vx(0x1b), [0])).exports.f())",
  simdRun(I, vsum + ".concat(vc(v32([5,6,7,8])), [0x41,1,0x1b,0x1a,0x41,4])"),
  VI_PRELUDE + "L(D(vi([0x7b],[0x7f],[0x41,0]))); L(T(function(){return vi([0x7b],[0x7f],vc(Array(16).fill(1)).concat([0x10,0])).exports.g()}))",
  VI_PRELUDE + "L(T(function(){return vi([],[0x7b],[0x10,0,0x1a,0x41,3]).exports.g()}))",
  VI_PRELUDE + "L(T(function(){return vi([0x7f,0x7b],[0x7f],[0x41,1].concat(vc(Array(16).fill(1)), [0x10,0])).exports.g()}))",
  VI_PRELUDE + "L(T(function(){return vi([],[0x7f,0x7b],[0x10,0,0x1a]).exports.g()}))",
  VI_PRELUDE + "L(T(function(){return vi([0x7f],[0x7f],[0x41,5,0x10,0]).exports.g()}))"
);
// Função exportada com `v128` no tipo: instancia e exporta normalmente (length vale a aridade), mas chamá-la do JS,
// direto, por `table.get` ou por `Function.prototype.call`, lança TypeError (JSToWasm do JSC). `WebAssembly.Function`
// não existe no bun 1.4.2, então não há caso de construtor.
// `TV(p, r, b)` monta um módulo com a função exportada "f" de tipo (p) -> (r) com corpo `b` e uma tabela "t" com ela.
const TV_PRELUDE =
  "var TV=function(p,r,b){var c=[0].concat(b,[0x0b]);return new WebAssembly.Instance(new WebAssembly.Module(mk(sec(1,[1,0x60,p.length].concat(p,[r.length],r)),sec(3,[1,0]),sec(4,[1,0x70,0,1]),sec(7,[2].concat(str(\"f\"),[0,0],str(\"t\"),[1,0])),sec(9,[1,0,0x41,0,0x0b,1,0]),sec(10,[1,c.length].concat(c)))))};";
const V16 = "[0xfd,12,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]";
add(
  TV_PRELUDE + "var i = TV([0x7b], [0x7f], [0x41, 1]); L(typeof i.exports.f); L(i.exports.f.length); L(T(function () { return i.exports.f() }))",
  TV_PRELUDE + "var i = TV([], [0x7b], " + V16 + "); L(i.exports.f.length); L(T(function () { return i.exports.f() }))",
  TV_PRELUDE + "var i = TV([0x7f, 0x7b], [0x7f], [0x20, 0]); L(i.exports.f.length); L(T(function () { return i.exports.f(1, 2) }))",
  TV_PRELUDE + "var i = TV([0x7b], [0x7f], [0x41, 1]); var f = i.exports.t.get(0); L(f === i.exports.f); L(T(function () { return f() }))",
  TV_PRELUDE + "var i = TV([0x7b], [0x7f], [0x41, 1]); L(T(function () { return i.exports.f.call(null, 1) })); L(T(function () { return i.exports.f.apply(null, [1]) }))",
  TV_PRELUDE + "var i = TV([0x7f], [0x7f], [0x20, 0]); L(i.exports.f(5)); L(i.exports.t.get(0)(6))",
  "L(typeof WebAssembly.Function)"
);
// GC, fatia i31 (prefixo 0xFB; o bun 1.4.2 também aceita struct/array/rec/sub no `validate`, ainda sem execução no porte):
// ref.i31 (28), i31.get_s (29), i31.get_u (30), ref.test/ref.cast (20..23) sobre i31 e funcref, ref.eq, ref.as_non_null.
const GCF = (body) => "L(new WebAssembly.Instance(new WebAssembly.Module(FN([], [0x7f], " + body + "))).exports.f())";
const GCT = (body) => "L(T(function () { return new WebAssembly.Instance(new WebAssembly.Module(FN([], [0x7f], " + body + "))).exports.f() }))";
add(
  GCF("[0x41, 5, 0xfb, 28, 0xfb, 29]"),
  GCF("[0x41, 0x7f, 0xfb, 28, 0xfb, 29]"),
  GCF("[0x41, 0x7f, 0xfb, 28, 0xfb, 30]"),
  GCF("[0x41, 0x80, 0x80, 0x80, 0x80, 0x78, 0xfb, 28, 0xfb, 30]"),
  GCF("[0x41, 0xff, 0xff, 0xff, 0xff, 0x03, 0xfb, 28, 0xfb, 29]"),
  GCF("[0x41, 5, 0xfb, 28, 0x41, 5, 0xfb, 28, 0xd3]"),
  GCF("[0x41, 5, 0xfb, 28, 0x41, 6, 0xfb, 28, 0xd3]"),
  GCF("[0xd0, 0x6d, 0xd0, 0x6d, 0xd3]"),
  GCF("[0xd0, 0x6d, 0x41, 1, 0xfb, 28, 0xd3]"),
  GCF("[0x41, 5, 0xfb, 28, 0xfb, 20, 0x6c]"),
  GCF("[0xd0, 0x6e, 0xfb, 20, 0x6c]"),
  GCF("[0xd0, 0x6e, 0xfb, 21, 0x6c]"),
  GCF("[0x41, 5, 0xfb, 28, 0xfb, 20, 0x6d]"),
  GCF("[0x41, 5, 0xfb, 28, 0xfb, 20, 0x6e]"),
  GCF("[0x41, 5, 0xfb, 28, 0xfb, 20, 0x6b]"),
  GCF("[0x41, 5, 0xfb, 28, 0xfb, 22, 0x6c, 0xfb, 29]"),
  GCF("[0xd0, 0x6e, 0xfb, 23, 0x6c, 0xd1]"),
  GCT("[0xd0, 0x6e, 0xfb, 22, 0x6c, 0xd1]"),
  GCT("[0xd0, 0x6c, 0xfb, 29]"),
  GCT("[0xd0, 0x6c, 0xfb, 30]"),
  GCT("[0xd0, 0x70, 0xd4, 0xd1]"),
  GCF("[0xd2, 0, 0xd4, 0xd1]"),
  GCF("[0xd2, 0, 0xfb, 20, 0x70]"),
  GCF("[0xd2, 0, 0xfb, 20, 0x00]"),
  GCF("[0xd2, 0, 0xfb, 20, 0x6e]".replace("0x6e", "0x73")),
  GCF("[0xd2, 0, 0xfb, 22, 0x70, 0xd1]"),
  GCF("[0xd0, 0x6f, 0xfb, 21, 0x6f]"),
  GCF("[0xd0, 0x6f, 0xfb, 20, 0x6f]"),
  "L(WebAssembly.validate(mk(sec(1, [2, 0x5f, 1, 0x7f, 1, 0x60, 0, 1, 0x7f]), sec(3, [1, 1]), sec(10, [1, 4, 0, 0x41, 1, 0x0b]))))",
  "L(WebAssembly.validate(mk(sec(1, [2, 0x5e, 0x7f, 1, 0x60, 0, 1, 0x7f]), sec(3, [1, 1]), sec(10, [1, 4, 0, 0x41, 1, 0x0b]))))",
  "L(WebAssembly.validate(mk(sec(1, [2, 0x4e, 1, 0x5f, 0, 0x60, 0, 1, 0x7f]), sec(3, [1, 1]), sec(10, [1, 4, 0, 0x41, 1, 0x0b]))))",
  "L(WebAssembly.validate(mk(sec(1, [2, 0x50, 0, 0x5f, 0, 0x60, 0, 1, 0x7f]), sec(3, [1, 1]), sec(10, [1, 4, 0, 0x41, 1, 0x0b]))))"
);
// GC, fatia 37 (struct, array, rec, sub, br_on_cast, conversões any/extern): `GM(tipos, k, n, r, locais, corpo)` monta uma
// instância cuja seção de tipos tem `k` entradas (`tipos`, os bytes), mais o tipo `() -> r` no índice `n`; "f" é a função.
// Prefixo 0xFB: struct.new 0, new_default 1, get 2, get_s 3, get_u 4, set 5; array.new 6, new_default 7, new_fixed 8,
// get 11, get_s 12, get_u 13, set 14, len 15, fill 16, copy 17; br_on_cast 24, br_on_cast_fail 25; any.convert_extern 26,
// extern.convert_any 27.
const GM_PRELUDE =
  "var GM=function(t,k,n,r,l,b){var c=l.concat(b,[0x0b]);return new WebAssembly.Instance(new WebAssembly.Module(mk(sec(1,[k+1].concat(t,[0x60,0,1,r])),sec(3,[1,n]),sec(7,[1].concat(str(\"f\"),[0,0])),sec(10,[1,c.length].concat(c)))))};";
const gm = (types, k, n, r, locals, body) =>
  GM_PRELUDE + "L(T(function(){return GM(" + [types, k, n, r, locals, body].map((x) => JSON.stringify(x)).join(",") + ").exports.f()}))";
// struct 0 {mut i32, mut i8}, array 1 {mut i32}, array 2 {mut i8}; função no índice 3.
const GT = [0x5f, 2, 0x7f, 1, 0x78, 1, 0x5e, 0x7f, 1, 0x5e, 0x78, 1];
const g3 = (locals, body) => gm(GT, 3, 3, 0x7f, locals, body);
const NOL = [0];
const LS = [1, 1, 0x63, 0];
const LA = [1, 2, 0x63, 1];
const LI = [2, 1, 0x63, 0, 1, 0x7f];
// Hierarquia: 0 = struct {i32} aberto, 1 = sub 0 {i32, i32}, 2 = struct {i32} irmão aberto, 3 = rec {struct A (ref null 3), struct B};
// 4 = array i32; função no índice 5, 4 entradas de tipo contando o rec como uma.
const HT = [0x50, 0, 0x5f, 1, 0x7f, 0, 0x50, 1, 0, 0x5f, 2, 0x7f, 0, 0x7f, 0, 0x5f, 1, 0x7f, 0, 0x5e, 0x7f, 1];
const gh = (locals, body) => gm(HT, 4, 4, 0x7f, locals, body);
// br_on_cast: desvia para o bloco de tipo `label`; o caminho que cai direto marca o local 1 com 11 (o local 0 guarda o objeto).
const brCast = (opcode, flags, ht1, ht2, label, make, fallthroughValue) =>
  [0x02].concat(label, make, [0xfb, opcode, flags, 0, ht1, ht2], fallthroughValue, [0x0b]);
add(
  // struct.new/get/set e campos packed.
  g3(NOL, [0x41, 5, 0x41, 0x7f, 0xfb, 0, 0, 0xfb, 2, 0, 0]),
  g3(NOL, [0x41, 5, 0x41, 0x7f, 0xfb, 0, 0, 0xfb, 2, 0, 1]),
  g3(NOL, [0x41, 5, 0x41, 0xff, 0x01, 0xfb, 0, 0, 0xfb, 3, 0, 1]),
  g3(NOL, [0x41, 5, 0x41, 0xff, 0x01, 0xfb, 0, 0, 0xfb, 4, 0, 1]),
  g3(NOL, [0x41, 5, 0x41, 0x80, 0x7f, 0xfb, 0, 0, 0xfb, 2, 0, 1]),
  g3(NOL, [0x41, 5, 0x41, 0x80, 0x02, 0xfb, 0, 0, 0xfb, 4, 0, 1]),
  g3(NOL, [0xfb, 1, 0, 0xfb, 2, 0, 0]),
  g3(NOL, [0xfb, 1, 0, 0xfb, 2, 0, 1]),
  g3(LS, [0xfb, 1, 0, 0x21, 0, 0x20, 0, 0x41, 9, 0xfb, 5, 0, 0, 0x20, 0, 0xfb, 2, 0, 0]),
  g3(LS, [0xfb, 1, 0, 0x21, 0, 0x20, 0, 0x41, 0xff, 0x01, 0xfb, 5, 0, 1, 0x20, 0, 0xfb, 4, 0, 1]),
  g3(LS, [0xfb, 1, 0, 0x21, 0, 0x20, 0, 0x41, 0xff, 0x01, 0xfb, 5, 0, 1, 0x20, 0, 0xfb, 3, 0, 1]),
  g3(NOL, [0xd0, 0, 0xfb, 2, 0, 0]),
  g3(NOL, [0xd0, 0, 0x41, 1, 0xfb, 5, 0, 0, 0x41, 0]),
  g3(NOL, [0xfb, 1, 0, 0xfb, 1, 0, 0xd3]),
  g3(LS, [0xfb, 1, 0, 0x21, 0, 0x20, 0, 0x20, 0, 0xd3]),
  // Estrutura imutável (campo imutável rejeita struct.set) e get em campo packed sem sinal.
  gm([0x5f, 1, 0x7f, 0], 1, 1, 0x7f, NOL, [0xfb, 1, 0, 0x41, 1, 0xfb, 5, 0, 0, 0x41, 0]),
  gm([0x5f, 1, 0x78, 1], 1, 1, 0x7f, NOL, [0xfb, 1, 0, 0xfb, 2, 0, 0]),
  // array.new/new_default/new_fixed/get/set/len/fill/copy e traps.
  g3(NOL, [0x41, 7, 0x41, 3, 0xfb, 6, 1, 0x41, 2, 0xfb, 11, 1]),
  g3(NOL, [0x41, 7, 0x41, 3, 0xfb, 6, 1, 0xfb, 15]),
  g3(NOL, [0x41, 7, 0x41, 3, 0xfb, 6, 1, 0x41, 3, 0xfb, 11, 1]),
  g3(NOL, [0x41, 7, 0x41, 3, 0xfb, 6, 1, 0x41, 0x7f, 0xfb, 11, 1]),
  g3(NOL, [0x41, 3, 0xfb, 7, 1, 0x41, 3, 0x41, 1, 0xfb, 14, 1, 0x41, 0]),
  g3(NOL, [0x41, 3, 0xfb, 7, 1, 0x41, 2, 0xfb, 11, 1]),
  g3(NOL, [0x41, 0, 0xfb, 7, 1, 0xfb, 15]),
  g3(NOL, [0x41, 0x80, 0x80, 0x80, 0x80, 0x7f, 0xfb, 7, 1, 0xfb, 15]),
  g3(NOL, [0x41, 0xff, 0xff, 0xff, 0xff, 0x0f, 0xfb, 7, 1, 0xfb, 15]),
  g3(NOL, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0x41, 2, 0xfb, 11, 1]),
  g3(NOL, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0xfb, 15]),
  g3(NOL, [0xfb, 8, 1, 0, 0xfb, 15]),
  g3(LA, [0x41, 4, 0xfb, 7, 1, 0x21, 0, 0x20, 0, 0x41, 1, 0x41, 9, 0x41, 2, 0xfb, 16, 1, 0x20, 0, 0x41, 2, 0xfb, 11, 1]),
  g3(LA, [0x41, 4, 0xfb, 7, 1, 0x21, 0, 0x20, 0, 0x41, 1, 0x41, 9, 0x41, 2, 0xfb, 16, 1, 0x20, 0, 0x41, 3, 0xfb, 11, 1]),
  g3(LA, [0x41, 4, 0xfb, 7, 1, 0x21, 0, 0x20, 0, 0x41, 3, 0x41, 9, 0x41, 2, 0xfb, 16, 1, 0x41, 0]),
  g3(LA, [0x41, 4, 0xfb, 7, 1, 0x21, 0, 0x20, 0, 0x41, 4, 0x41, 9, 0x41, 0, 0xfb, 16, 1, 0x41, 7]),
  g3(LA, [0x41, 4, 0xfb, 7, 1, 0x21, 0, 0x20, 0, 0x41, 5, 0x41, 9, 0x41, 0, 0xfb, 16, 1, 0x41, 7]),
  // array.copy: dentro do mesmo array com sobreposição, entre dois, e fora do intervalo.
  g3(LA, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0x21, 0, 0x20, 0, 0x41, 1, 0x20, 0, 0x41, 0, 0x41, 2, 0xfb, 17, 1, 1, 0x20, 0, 0x41, 2, 0xfb, 11, 1]),
  g3(LA, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0x21, 0, 0x20, 0, 0x41, 0, 0x20, 0, 0x41, 1, 0x41, 2, 0xfb, 17, 1, 1, 0x20, 0, 0x41, 0, 0xfb, 11, 1]),
  g3(LA, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0x21, 0, 0x20, 0, 0x41, 2, 0x20, 0, 0x41, 0, 0x41, 2, 0xfb, 17, 1, 1, 0x41, 0]),
  g3(LA, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0x21, 0, 0x20, 0, 0x41, 0, 0x20, 0, 0x41, 2, 0x41, 2, 0xfb, 17, 1, 1, 0x41, 0]),
  g3(LA, [0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 8, 1, 3, 0x21, 0, 0x20, 0, 0x41, 3, 0x20, 0, 0x41, 0, 0x41, 0, 0xfb, 17, 1, 1, 0x41, 7]),
  // Array packed: truncamento e extensão com e sem sinal.
  g3(NOL, [0x41, 0xff, 0x01, 0x41, 2, 0xfb, 6, 2, 0x41, 0, 0xfb, 12, 2]),
  g3(NOL, [0x41, 0xff, 0x01, 0x41, 2, 0xfb, 6, 2, 0x41, 0, 0xfb, 13, 2]),
  g3(NOL, [0x41, 0x80, 0x80, 0x04, 0x41, 2, 0xfb, 6, 2, 0x41, 1, 0xfb, 13, 2]),
  g3(NOL, [0x41, 3, 0xfb, 7, 2, 0x41, 0, 0xfb, 11, 2]),
  g3(NOL, [0xd0, 1, 0xfb, 15]),
  g3(NOL, [0xd0, 1, 0x41, 0, 0xfb, 11, 1]),
  g3(NOL, [0xd0, 1, 0x41, 0, 0x41, 0, 0xfb, 14, 1, 0x41, 0]),
  // Rec groups e sub: ref.test/ref.cast entre pai e filho, irmãos e tipos abstratos.
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 0, 1, 0xfb, 20, 0]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 1]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 2]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 0, 1, 0xfb, 20, 2]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 0, 1, 0xfb, 22, 0, 0xfb, 2, 0, 0]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 0, 1, 0xfb, 2, 1, 1]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 22, 1, 0xfb, 2, 1, 0]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 22, 2, 0xfb, 2, 2, 0]),
  gh(NOL, [0xd0, 0, 0xfb, 22, 0, 0xfb, 2, 0, 0]),
  gh(NOL, [0xd0, 0, 0xfb, 23, 0, 0xd1]),
  gh(NOL, [0xd0, 0, 0xfb, 21, 0]),
  gh(NOL, [0xd0, 0, 0xfb, 20, 0]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x6b]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x6a]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x6d]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x6e]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x6c]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x6f]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x70]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0xfb, 20, 0x71]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 6, 3, 0xfb, 20, 0x6a]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 6, 3, 0xfb, 20, 0x6b]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 6, 3, 0xfb, 20, 0x6d]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 6, 3, 0xfb, 20, 3]),
  gh(NOL, [0x41, 1, 0x41, 2, 0xfb, 6, 3, 0xfb, 20, 0]),
  gh(NOL, [0xd0, 0x6e, 0xfb, 20, 0x6e]),
  gh(NOL, [0xd0, 0x6e, 0xfb, 21, 0x6e]),
  gh(NOL, [0x41, 7, 0xfb, 28, 0xfb, 20, 0x6b]),
  // Igualdade: o mesmo objeto, dois iguais, struct contra i31.
  gh(LS, [0x41, 1, 0xfb, 0, 0, 0x41, 1, 0xfb, 0, 0, 0xd3]),
  gh(NOL, [0x41, 1, 0xfb, 0, 0, 0x41, 1, 0xfb, 28, 0xd3]),
  // Rec group com referência a si mesmo.
  gm([0x4e, 2, 0x5f, 1, 0x63, 1, 1, 0x5f, 1, 0x7f, 1], 1, 2, 0x7f, NOL, [0xfb, 1, 1, 0xfb, 2, 1, 0]),
  gm([0x4e, 2, 0x5f, 1, 0x63, 1, 1, 0x5f, 1, 0x7f, 1], 1, 2, 0x7f, NOL, [0xfb, 1, 0, 0xfb, 2, 0, 0, 0xd1]),
  // any.convert_extern e extern.convert_any.
  g3(NOL, [0xd0, 0x6f, 0xfb, 26, 0xd1]),
  g3(NOL, [0xd0, 0x6e, 0xfb, 27, 0xd1]),
  g3(NOL, [0x41, 5, 0xfb, 28, 0xfb, 27, 0xfb, 26, 0xfb, 29]),
  g3(NOL, [0x41, 5, 0xfb, 28, 0xfb, 27, 0xfb, 26, 0xfb, 20, 0x6c]),
  g3(NOL, [0xfb, 1, 0, 0xfb, 27, 0xfb, 26, 0xfb, 22, 0, 0xfb, 2, 0, 0]),
  g3(NOL, [0xfb, 1, 0, 0xfb, 27, 0xfb, 20, 0x6f]),
  g3(NOL, [0xfb, 1, 0, 0xfb, 27, 0xfb, 26, 0xfb, 20, 0]),
  // br_on_cast e br_on_cast_fail: o local 1 vira 11 quando o desvio não é tomado (casos de bloco com objeto de rótulo).
  gh(LI, brCast(24, 0, 0, 1, [0x64, 1], [0x41, 1, 0x41, 2, 0xfb, 0, 1], [0x1a, 0x41, 11, 0x21, 1, 0x41, 1, 0x41, 2, 0xfb, 0, 1]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 0, 0, 1, [0x64, 1], [0x41, 1, 0xfb, 0, 0], [0x1a, 0x41, 11, 0x21, 1, 0x41, 1, 0x41, 2, 0xfb, 0, 1]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(25, 0, 0, 1, [0x64, 0], [0x41, 1, 0x41, 2, 0xfb, 0, 1], [0x41, 11, 0x21, 1]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(25, 0, 0, 1, [0x64, 0], [0x41, 1, 0xfb, 0, 0], [0x41, 11, 0x21, 1]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 3, 0, 0, [0x63, 0], [0xd0, 0], [0x1a, 0x41, 11, 0x21, 1, 0xd0, 0]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 1, 0, 0, [0x63, 0], [0xd0, 0], [0x1a, 0x41, 11, 0x21, 1, 0xd0, 0]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(25, 3, 0, 0, [0x63, 0], [0xd0, 0], [0x41, 11, 0x21, 1]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(25, 1, 0, 0, [0x63, 0], [0xd0, 0], [0x41, 11, 0x21, 1]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 0, 0x6e, 0, [0x64, 0], [0x41, 1, 0xfb, 0, 0], [0x1a, 0x41, 11, 0x21, 1, 0x41, 1, 0xfb, 0, 0]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 0, 0x6e, 0, [0x64, 0], [0x41, 1, 0xfb, 28], [0x1a, 0x41, 11, 0x21, 1, 0x41, 1, 0xfb, 0, 0]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 0, 0x6e, 0x6c, [0x64, 0x6c], [0x41, 1, 0xfb, 28], [0x1a, 0x41, 11, 0x21, 1, 0x41, 1, 0xfb, 28]).concat([0x1a, 0x20, 1])),
  gh(LI, brCast(24, 0, 0x6e, 0x6c, [0x64, 0x6c], [0x41, 1, 0xfb, 0, 0], [0x1a, 0x41, 11, 0x21, 1, 0x41, 1, 0xfb, 28]).concat([0x1a, 0x20, 1])),
  // Validação de GC com tipos errados.
  gm([0x5f, 1, 0x7f, 0], 1, 1, 0x7f, NOL, [0xfb, 1, 0, 0xfb, 2, 0, 1]),
  gm([0x5f, 1, 0x7f, 0], 1, 1, 0x7f, NOL, [0xfb, 1, 1, 0xfb, 2, 0, 0]),
  gm([0x5f, 1, 0x78, 1], 1, 1, 0x7f, NOL, [0xfb, 1, 0, 0xfb, 2, 0, 0]),
  gm([0x50, 0, 0x5f, 1, 0x7f, 0, 0x50, 1, 0, 0x5f, 1, 0x78, 0], 2, 2, 0x7f, NOL, [0x41, 0]),
  gm([0x4f, 0, 0x5f, 1, 0x7f, 0, 0x50, 1, 0, 0x5f, 1, 0x7f, 0], 2, 2, 0x7f, NOL, [0x41, 0]),
  gm([0x50, 0, 0x5f, 1, 0x7f, 0, 0x50, 1, 0, 0x5f, 1, 0x7f, 0], 2, 2, 0x7f, NOL, [0x41, 0]),
  gm([0x50, 0, 0x5f, 1, 0x7f, 1, 0x50, 1, 0, 0x5f, 1, 0x7f, 0], 2, 2, 0x7f, NOL, [0x41, 0]),
  gm([0x50, 0, 0x5f, 1, 0x7f, 0, 0x50, 1, 0, 0x5f, 2, 0x7f, 0, 0x7f, 0], 2, 2, 0x7f, NOL, [0x41, 0])
);
// array.new_data/new_elem/init_data/init_elem (0xFB 9, 10, 18, 19) e o limite de bytes de array (2^30). `GD(tipos, k, locais,
// corpo, antes, depois)`: como GM com retorno i32; `antes` são as seções entre export e código (elem 9, datacount 12) e
// `depois` as depois do código (data 11).
const GD_PRELUDE =
  "var GD=function(t,k,l,b,p,q){var c=l.concat(b,[0x0b]);return new WebAssembly.Instance(new WebAssembly.Module(mk.apply(null,[sec(1,[k+1].concat(t,[0x60,0,1,0x7f])),sec(3,[1,k]),sec(7,[1].concat(str(\"f\"),[0,0]))].concat(p,[sec(10,[1,c.length].concat(c))],q))))};";
const gd = (types, k, locals, body, before, after) =>
  GD_PRELUDE + "L(T(function(){return GD(" + [types, k, locals, body, before, after].map((x) => JSON.stringify(x)).join(",") + ").exports.f()}))";
const DT = [0x5e, 0x78, 1, 0x5e, 0x7f, 1]; // 0 = array i8, 1 = array i32
const DSEC = [[0x0c, 1, 1]]; // datacount
const DATA1 = [[0x0b, 8, 1, 1, 5, 1, 2, 3, 4, 5]];
const DATA32 = [[0x0b, 11, 1, 1, 8, 1, 0, 0, 0, 2, 0, 0, 0]];
const EL = [[0x09, 13, 1, 5, 0x70, 3, 0xd2, 0, 0x0b, 0xd0, 0x70, 0x0b, 0xd2, 0, 0x0b]]; // passivo: ref.func 0, ref.null, ref.func 0
const LNEW = [1, 1, 0x63, 0];
// Array de i8 com 5 posições no local 0 (para init_data).
const mk5 = [0x41, 5, 0xfb, 7, 0, 0x21, 0];
const dataCase = (body, locals, data, types, k) => gd(types || DT, k || 2, locals || NOL, body, DSEC, data || DATA1);
const elemCase = (body, locals) => gd([0x5e, 0x70, 1], 1, locals || NOL, body, EL, []);
add(
  dataCase([0x41, 1, 0x41, 3, 0xfb, 9, 0, 0, 0x41, 2, 0xfb, 13, 0]),
  dataCase([0x41, 1, 0x41, 3, 0xfb, 9, 0, 0, 0xfb, 15]),
  dataCase([0x41, 0, 0x41, 2, 0xfb, 9, 1, 0, 0x41, 1, 0xfb, 11, 1], NOL, DATA32),
  dataCase([0x41, 3, 0x41, 5, 0xfb, 9, 0, 0, 0xfb, 15]),
  dataCase([0x41, 0, 0x41, 3, 0xfb, 9, 1, 0, 0xfb, 15], NOL, DATA32),
  dataCase([0xfc, 9, 0, 0x41, 0, 0x41, 1, 0xfb, 9, 0, 0, 0xfb, 15]),
  dataCase([0xfc, 9, 0, 0x41, 0, 0x41, 0, 0xfb, 9, 0, 0, 0xfb, 15]),
  dataCase([0x41, 0, 0x41, 0x7f, 0xfb, 9, 0, 0, 0xfb, 15]),
  dataCase(mk5.concat([0x20, 0, 0x41, 1, 0x41, 2, 0x41, 3, 0xfb, 18, 0, 0, 0x20, 0, 0x41, 2, 0xfb, 13, 0]), LNEW),
  dataCase(mk5.concat([0x20, 0, 0x41, 3, 0x41, 0, 0x41, 3, 0xfb, 18, 0, 0, 0x41, 1]), LNEW),
  dataCase(mk5.concat([0x20, 0, 0x41, 0, 0x41, 3, 0x41, 3, 0xfb, 18, 0, 0, 0x41, 1]), LNEW),
  dataCase([0xd0, 0, 0x41, 0, 0x41, 0, 0x41, 0, 0xfb, 18, 0, 0, 0x41, 1]),
  dataCase(mk5.concat([0x20, 0, 0x41, 5, 0x41, 5, 0x41, 0, 0xfb, 18, 0, 0, 0x41, 1]), LNEW),
  dataCase(mk5.concat([0xfc, 9, 0, 0x20, 0, 0x41, 0, 0x41, 0, 0x41, 1, 0xfb, 18, 0, 0, 0x41, 1]), LNEW),
  dataCase([0x41, 3, 0xfb, 7, 1, 0x21, 0, 0x20, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0xfb, 18, 1, 0, 0x20, 0, 0x41, 1, 0xfb, 11, 1], [1, 1, 0x63, 1], DATA32),
  elemCase([0x41, 0, 0x41, 3, 0xfb, 10, 0, 0, 0xfb, 15]),
  elemCase([0x41, 0, 0x41, 3, 0xfb, 10, 0, 0, 0x41, 1, 0xfb, 11, 0, 0xd1]),
  elemCase([0x41, 0, 0x41, 3, 0xfb, 10, 0, 0, 0x41, 0, 0xfb, 11, 0, 0xd1]),
  elemCase([0x41, 1, 0x41, 2, 0xfb, 10, 0, 0, 0x41, 0, 0xfb, 11, 0, 0xd1]),
  elemCase([0x41, 2, 0x41, 2, 0xfb, 10, 0, 0, 0xfb, 15]),
  elemCase([0xfc, 13, 0, 0x41, 0, 0x41, 1, 0xfb, 10, 0, 0, 0xfb, 15]),
  elemCase([0xfc, 13, 0, 0x41, 0, 0x41, 0, 0xfb, 10, 0, 0, 0xfb, 15]),
  elemCase([0x41, 3, 0xfb, 7, 0, 0x21, 0, 0x20, 0, 0x41, 0, 0x41, 1, 0x41, 1, 0xfb, 19, 0, 0, 0x20, 0, 0x41, 0, 0xfb, 11, 0, 0xd1], LNEW),
  elemCase([0x41, 3, 0xfb, 7, 0, 0x21, 0, 0x20, 0, 0x41, 1, 0x41, 0, 0x41, 3, 0xfb, 19, 0, 0, 0x41, 1], LNEW),
  elemCase([0x41, 3, 0xfb, 7, 0, 0x21, 0, 0x20, 0, 0x41, 0, 0x41, 1, 0x41, 3, 0xfb, 19, 0, 0, 0x41, 1], LNEW),
  elemCase([0xd0, 0, 0x41, 0, 0x41, 0, 0x41, 0, 0xfb, 19, 0, 0, 0x41, 1]),
  elemCase([0x41, 3, 0xfb, 7, 0, 0x21, 0, 0x20, 0, 0x41, 3, 0x41, 3, 0x41, 0, 0xfb, 19, 0, 0, 0x41, 1], LNEW),
  elemCase([0x41, 3, 0xfb, 7, 0, 0x21, 0, 0xfc, 13, 0, 0x20, 0, 0x41, 0, 0x41, 0, 0x41, 1, 0xfb, 19, 0, 0, 0x41, 1], LNEW),
  // Limite de bytes do array: 2^30 bytes passam (i8 2^30 elementos, i32 2^28, medido à parte: o golden só guarda os
  // que falham, porque alocar 1 GiB num teste é pesado), um elemento a mais falha.
  gd(DT, 2, NOL, [0x41, 0x81, 0x80, 0x80, 0x80, 0x04, 0xfb, 7, 0, 0xfb, 15], [], []),
  gd(DT, 2, NOL, [0x41, 0x81, 0x80, 0x80, 0x80, 0x01, 0xfb, 7, 1, 0xfb, 15], [], [])
);
// Objeto GC no JS (fatia 37c). `GG()` monta um módulo com struct 0 {mut i32} e as funções exportadas: f () -> (ref null 0)
// (struct.new_default), g (ref null 0) -> i32 (struct.get), h (ref null 0) -> (ref null 0) (identidade), i () -> i31ref
// (ref.i31 5), a () -> anyref (struct.new_default), c (structref) -> i32 (ref.cast + struct.get), d (arrayref) -> i32 (0).
const GG_PRELUDE =
  "var GG=function(){var ty=[0x5f,1,0x7f,1,0x60,0,1,0x63,0,0x60,1,0x63,0,1,0x7f,0x60,1,0x63,0,1,0x63,0,0x60,0,1,0x63,0x6c,0x60,0,1,0x6e,0x60,1,0x63,0x6b,1,0x7f,0x60,1,0x63,0x6a,1,0x7f];" +
  "var bodies=[[0xfb,1,0],[0x20,0,0xfb,2,0,0],[0x20,0],[0x41,5,0xfb,28],[0xfb,1,0],[0x20,0,0xfb,23,0,0xfb,2,0,0],[0x41,0]];" +
  "var fi=[1,2,3,4,5,6,7];var names=['f','g','h','i','a','c','d'];var ex=[names.length];" +
  "for(var q=0;q<names.length;q++)ex=ex.concat(str(names[q]),[0,q]);" +
  "var code=[bodies.length];bodies.forEach(function(b){var c=[0].concat(b,[0x0b]);code=code.concat([c.length],c)});" +
  "return new WebAssembly.Instance(new WebAssembly.Module(mk(sec(1,[8].concat(ty)),sec(3,[fi.length].concat(fi)),sec(7,ex),sec(10,code))))};";
const gg = (body) => GG_PRELUDE + body;
add(
  gg("var e=GG().exports;var o=e.f();L(typeof o);L(Object.getPrototypeOf(o));L(Object.prototype.toString.call(o));L(Object.keys(o).length);L(Reflect.ownKeys(o).length);L(Object.isFrozen(o));L(Object.isSealed(o));L(Object.isExtensible(o));L(JSON.stringify(o))"),
  gg("var e=GG().exports;var o=e.f();L(T(function(){o.x=1}));L(T(function(){o[0]=1}));L(T(function(){'use strict';o.x=1}));L(T(function(){Object.setPrototypeOf(o,{})}));L(T(function(){Object.setPrototypeOf(o,null)}));L(T(function(){o.__proto__={}}))"),
  gg("var e=GG().exports;var o=e.f();L(T(function(){return String(o)}));L(T(function(){return `${o}`}));L(T(function(){return o+1}));L(T(function(){return +o}));L('x' in o);L(o instanceof Object);L(o.x);L(o[0]);L(o.toString)"),
  gg("var e=GG().exports;var o=e.f();L(T(function(){return delete o.x}));L(T(function(){return Object.defineProperty(o,'x',{value:1})}));L(T(function(){return Reflect.set(o,'x',1)}));L(T(function(){return Reflect.defineProperty(o,'x',{value:1})}));L(T(function(){return Reflect.deleteProperty(o,'x')}));L(T(function(){return Reflect.preventExtensions(o)}));L(Reflect.getPrototypeOf(o));L(T(function(){return Reflect.setPrototypeOf(o,null)}));L(T(function(){return Reflect.setPrototypeOf(o,{})}));L(Object.getOwnPropertyDescriptor(o,'x'));L(Object.getOwnPropertyNames(o).length)"),
  gg("var e=GG().exports;var o=e.f();L(T(function(){return Object.freeze(o)===o}));L(T(function(){return Object.seal(o)===o}));L(T(function(){return Object.preventExtensions(o)===o}));L(T(function(){return Object.isFrozen(Object.freeze(e.f()))}))"),
  gg("var e=GG().exports;var o=e.f();var p=e.f();L(o===p);L(o===o);L(e.h(o)===o);L(e.h(p)===p);L(e.h(p)===o);L(e.h(null));L(e.g(o));L(Object.is(e.h(o),o))"),
  gg("var e=GG().exports;var o=e.f();L(T(function(){return e.g(1)}));L(T(function(){return e.g({})}));L(T(function(){return e.g('a')}));L(T(function(){return e.g(undefined)}));L(T(function(){return e.g()}));L(T(function(){return e.g(null)}));L(T(function(){return e.h(1.5)}));L(T(function(){return e.g(e.g)}))"),
  gg("var e=GG().exports;var o=e.f();L(e.c(o));L(T(function(){return e.c(null)}));L(T(function(){return e.c(1)}));L(T(function(){return e.d(o)}));L(T(function(){return e.d(null)}));L(T(function(){return e.d(e.a())}))"),
  gg("var e=GG().exports;var i=e.i();L(i);L(typeof i);L(e.a()!==e.a());L(typeof e.a());L(Object.getPrototypeOf(e.a()));L(T(function(){return e.d(i)}));L(T(function(){return e.c(i)}))"),
  gg("var e=GG().exports;var o=e.f();var m=new Map();m.set(o,1);L(m.get(o));L(m.get(e.f()));var s=new Set([o,o]);L(s.size);var w=new WeakMap();w.set(o,2);L(w.get(o));var a=[o];L(a.indexOf(o));L(a.includes(o))"),
  gg("var e=GG().exports;var o=e.f();L(Object.entries(o).length);L(Object.assign({},o));L(Object.getOwnPropertySymbols(o).length);var n=0;for(var k in o)n++;L(n);L(Object.hasOwn(o,'x'));L(Object.prototype.hasOwnProperty.call(o,'x'));L(Object.prototype.isPrototypeOf.call(o,o));L(structuredClone===undefined?0:T(function(){return structuredClone(o)}))")
);
// Valor não GC em anyref/externref e i31 negativo (fatia 37d). `XX()` monta um módulo com: a anyref->anyref, x externref->externref,
// i () -> (ref i31) (ref.i31 -1), c externref->anyref (any.convert_extern), e anyref->externref (extern.convert_any).
const XX_PRELUDE =
  "var XX=function(){var ty=[0x5f,1,0x7f,1,0x60,1,0x6e,1,0x6e,0x60,1,0x6f,1,0x6f,0x60,0,1,0x64,0x6c,0x60,1,0x6f,1,0x6e,0x60,1,0x6e,1,0x6f];" +
  "var bodies=[[0x20,0],[0x20,0],[0x41,0x7f,0xfb,28],[0x20,0,0xfb,0x1a],[0x20,0,0xfb,0x1b]];" +
  "var fi=[1,2,3,4,5];var names=['a','x','i','c','e'];var ex=[names.length];" +
  "for(var q=0;q<names.length;q++)ex=ex.concat(str(names[q]),[0,q]);" +
  "var code=[bodies.length];bodies.forEach(function(b){var c=[0].concat(b,[0x0b]);code=code.concat([c.length],c)});" +
  "return new WebAssembly.Instance(new WebAssembly.Module(mk(sec(1,[6].concat(ty)),sec(3,[fi.length].concat(fi)),sec(7,ex),sec(10,code))))};";
const xx = (body) => XX_PRELUDE + body;
add(
  xx("var e=XX().exports;var o={};var s=Symbol.iterator;['a','x','c','e'].forEach(function(n){L(n);L(e[n](1));L(e[n](undefined));L(e[n]('a'));L(e[n](null));L(e[n](1.5));L(e[n](true));L(e[n](s)===s);L(e[n](o)===o);L(e[n](e.a)===e.a);L(e[n](0n))})"),
  xx("var e=XX().exports;L(e.i());L(Object.is(e.i(),-1));L(typeof e.i())"),
  xx("var e=XX().exports;var o={};L(e.e(e.c(o))===o);L(e.c(e.e(o))===o);L(e.x(e.x(2**40)));L(Object.is(e.x(-0),-0));L(Object.is(e.a(NaN),NaN))"),
  xx("var t=new WebAssembly.Table({element:'externref',initial:2},7);var g=new WebAssembly.Global({value:'externref'},'q');var o={};L(t.get(0));L(t.get(1));t.set(0,o);L(t.get(0)===o);L(g.value);g=new WebAssembly.Global({value:'externref'},o);L(g.value===o);L(T(function(){return t.set(1,undefined)}));L(t.get(1))")
);
// Objeto GC opaco dentro de Table/Global externref (fatia 37e): o dono vai no próprio objeto opaco.
add(
  gg("var e=GG().exports;var o=e.f();var t=new WebAssembly.Table({element:'externref',initial:2},o);L(t.get(0)===o);L(t.get(1)===o);t.set(0,e.f());L(t.get(0)===o);L(t.get(0)===t.get(1));L(typeof t.get(0));L(e.h(t.get(1))===o)"),
  gg("var e=GG().exports;var o=e.f();var g=new WebAssembly.Global({value:'externref',mutable:true},o);L(g.value===o);var p=e.f();g.value=p;L(g.value===p);L(g.value===o);L(e.g(g.value));L(Object.is(g.valueOf(),p))"),
  gg("var e=GG().exports;var o=e.f();var t=new WebAssembly.Table({element:'anyref',initial:1});L(T(function(){t.set(0,o);return t.get(0)===o}));L(T(function(){t.set(0,{});return typeof t.get(0)}));L(T(function(){return new WebAssembly.Global({value:'anyref'},o).value===o}));var t2=new WebAssembly.Table({element:'anyref',initial:1},o);L(t2.get(0)===o)"),
  gg("var e=GG().exports;var o=e.f();var t=new WebAssembly.Table({element:'externref',initial:0});L(t.grow(2,o));L(t.get(1)===o);L(t.length);var m=new Map();m.set(t.get(0),'v');L(m.get(o))")
);
// WebAssembly.JSTag (medido no bun 1.4.2): acessor, instância de Tag, importável como tag (param externref); uma
// exceção de JS lançada por uma importação é capturada por `try_table (catch 0)` com a JSTag, e `throw` com ela
// volta ao JS como o valor original. CATCH: import m.f ()->(), tag m.t (externref), g()->externref com o try_table.
// THROW: import tag m.t (externref), g(externref) faz `throw 0`.
const JSTAG_PRELUDE =
  "var CATCH=new Uint8Array([0,97,115,109,1,0,0,0,1,12,3,96,0,0,96,1,111,0,96,0,1,111,2,14,2,1,109,1,102,0,0,1,109,1,116,4,0,1,3,2,1,2,7,5,1,1,103,0,1,10,18,1,16,0,2,111,31,64,1,0,0,0,16,0,11,208,111,11,11]);" +
  "var THROW=new Uint8Array([0,97,115,109,1,0,0,0,1,8,2,96,0,0,96,1,111,0,2,8,1,1,109,1,116,4,0,1,3,2,1,1,7,5,1,1,103,0,0,10,8,1,6,0,32,0,8,0,11]);" +
  "var JT=WebAssembly.JSTag;function CI(f,t){return new WebAssembly.Instance(new WebAssembly.Module(CATCH),{m:{f:f,t:t}})};function TI(t){return new WebAssembly.Instance(new WebAssembly.Module(THROW),{m:{t:t}})};";
const jt = (body) => JSTAG_PRELUDE + body;
add(
  "L(JSON.stringify(Object.getOwnPropertyDescriptor(WebAssembly, 'JSTag'), function(k, v) { return typeof v === 'function' ? 'F:' + v.name + ':' + v.length : v })); L(JT === WebAssembly.JSTag); L(JT instanceof WebAssembly.Tag); L(Object.getPrototypeOf(JT) === WebAssembly.Tag.prototype); L(Object.prototype.toString.call(JT)); L(typeof JT.type); L(Reflect.ownKeys(JT).length)",
  jt("var o = { a: 1 }; var r = CI(function() { throw o }, JT).exports.g(); L(r === o); L(CI(function() { throw 5 }, JT).exports.g()); L(CI(function() { throw null }, JT).exports.g()); L(CI(function() { throw 's' }, JT).exports.g()); L(CI(function() {}, JT).exports.g()); L(CI(function() { throw new RangeError('x') }, JT).exports.g() instanceof RangeError)"),
  jt("var o = { a: 1 }; var i = CI(function() { throw o }, new WebAssembly.Tag({ parameters: ['externref'] })); L(T(function() { try { i.exports.g() } catch (e) { return e === o } }))"),
  jt("var o = { a: 1 }; var i = TI(JT); [o, 5, null, undefined, 's'].forEach(function(v) { try { i.exports.g(v) } catch (e) { L(e === v); L(e instanceof WebAssembly.Exception) } })"),
  jt("var t = new WebAssembly.Tag({ parameters: ['externref'] }); var o = { a: 1 }; try { TI(t).exports.g(o) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(t)); L(e.is(JT)); L(e.getArg(t, 0) === o) }"),
  jt("L(T(function() { return new WebAssembly.Exception(JT, [{}]) })); L(T(function() { return new WebAssembly.Exception(JT, []) })); L(T(function() { return new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,1,4,1,96,0,0,2,8,1,1,109,1,116,4,0,0])), { m: { t: JT } }) })); L(T(function() { return new WebAssembly.Instance(new WebAssembly.Module(THROW), { m: { t: {} } }) }))")
);
// JSPI (medido no bun 1.4.2): forma estática, mensagens de argumento, SuspendError, e um módulo IMP (g(x) = m.f(x))
// com importação `Suspending` de função assíncrona já assentada. Suspensão sobre promessa pendente e o erro de
// frame JS entre `Suspending` e `promising` ficam fora do golden (ver LACUNA em src/runtime/js_web_assembly_jspi.rs).
add(
  "L(JSON.stringify(Reflect.ownKeys(WebAssembly.promising))); L(JSON.stringify(Reflect.ownKeys(WebAssembly.Suspending))); L(JSON.stringify(Reflect.ownKeys(WebAssembly.Suspending.prototype))); L(JSON.stringify(Reflect.ownKeys(WebAssembly.SuspendError))); L(JSON.stringify(Reflect.ownKeys(WebAssembly.SuspendError.prototype)))",
  "['promising', 'Suspending', 'SuspendError'].forEach(function(k) { var d = Object.getOwnPropertyDescriptor(WebAssembly, k); L(k + ':' + typeof d.value + ':' + d.writable + ':' + d.enumerable + ':' + d.configurable + ':' + WebAssembly[k].length + ':' + WebAssembly[k].name) })",
  "L(Object.getPrototypeOf(WebAssembly.SuspendError.prototype) === Error.prototype); L(WebAssembly.SuspendError.prototype.name); L(new WebAssembly.SuspendError('x').message); L(WebAssembly.SuspendError('y').message); L(new WebAssembly.SuspendError('x') instanceof Error); L(Object.prototype.toString.call(new WebAssembly.SuspendError('x'))); L(Object.getPrototypeOf(WebAssembly.SuspendError) === Error)",
  "L(T(() => WebAssembly.promising(1))); L(T(() => WebAssembly.promising())); L(T(() => WebAssembly.promising(() => 1))); L(T(() => new WebAssembly.Suspending(1))); L(T(() => new WebAssembly.Suspending())); L(T(() => WebAssembly.Suspending(() => 1)))",
  "var s = new WebAssembly.Suspending(() => 1); L(typeof s); L(s instanceof Function); L(s.name); L(s.length); L(JSON.stringify(Object.getOwnPropertyNames(s))); L(Object.getPrototypeOf(s) === Function.prototype); L(Object.prototype.toString.call(s)); L(T(() => s())); L(T(() => s.call(null, 1)))",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: x => x * 2 } }); var p = WebAssembly.promising(i.exports.g); L(typeof p); L(p.length); L(p.name); L(p === i.exports.g); L(JSON.stringify(Object.getOwnPropertyNames(p))); var r = p(21); L(r instanceof Promise); P(r, 'sync')",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: new WebAssembly.Suspending(async x => x + 1) } }); L(T(() => i.exports.g(1))); P(WebAssembly.promising(i.exports.g)(1), 'async')",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: new WebAssembly.Suspending(x => x + 2) } }); P(WebAssembly.promising(i.exports.g)(1), 'plain')",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(IMP), { m: { f: new WebAssembly.Suspending(async x => { throw new RangeError('boom') }) } }); P(WebAssembly.promising(i.exports.g)(1), 'reject')",
  "var i = new WebAssembly.Instance(new WebAssembly.Module(TRAP)); P(WebAssembly.promising(i.exports.t)(), 'trap'); L(T(() => WebAssembly.promising(i.exports.t)()))"
);
// JSPI com suspensão real (medido no bun 1.4.2). Sem temporizadores: toda promessa pendente é assentada por
// microtarefas ou por funções de resolução capturadas, então a ordem é determinística. Módulos montados por JM(tipos,
// importações, funções, exportações, seções extras, start): tipos [[params],[results]], importações [módulo, nome, tipo],
// funções [tipo, corpo, locais], exportações [nome, índice, tipo de exportação]. As importações ocupam os primeiros
// índices de função; X(módulo, f, h) instancia com `m.f` e `m.h`.
const JSPI_PRELUDE = [
  "var i32=0x7f,i64=0x7e,f32=0x7d,f64=0x7c,xr=0x6f;",
  "var S=function(f){return new WebAssembly.Suspending(f)};var PR=WebAssembly.promising;",
  "var R=function(v){return typeof v==='bigint'?v+'n':JSON.stringify(v)};",
  "var Q=function(p,l){p.then(function(v){L(l+':ok:'+R(v))},function(e){L(l+':err:'+E(e))})};",
  "var JM=function(t,im,fs,ex,ms,st){var ty=[t.length];t.forEach(function(x){ty.push(0x60,x[0].length);ty=ty.concat(x[0],[x[1].length],x[1])});",
  "var imp=[im.length];im.forEach(function(x){imp=imp.concat(str(x[0]),str(x[1]),[0,x[2]])});",
  "var fn=[fs.length];fs.forEach(function(f){fn.push(f[0])});",
  "var exp=[ex.length];ex.forEach(function(x){exp=exp.concat(str(x[0]),[x[2]||0,x[1]])});",
  "var cd=[fs.length];fs.forEach(function(f){var l=f[2]||[];var b=[l.length/2].concat(l,f[1],[0x0b]);cd.push(b.length);cd=cd.concat(b)});",
  "return mk(sec(1,ty),im.length?sec(2,imp):[],sec(3,fn),ms||[],sec(7,exp),st===undefined?[]:sec(8,[st]),sec(10,cd))};",
  "var X=function(m,f,h){return new WebAssembly.Instance(new WebAssembly.Module(m),{m:{f:f,h:h}}).exports};",
  "var PT=function(t){return JM([[[t],[t]]],[['m','f',0]],[[0,[0x20,0,0x10,0]]],[['g',1]])};",
  "var TWO=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x20,0,0x10,0,0x10,0]]],[['g',1]]);",
  "var REC=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x20,0,0x45,0x04,0x7f,0x41,0,0x10,0,0x05,0x20,0,0x41,1,0x6b,0x10,1,0x20,0,0x10,0,0x6a,0x0b]]],[['g',1]]);",
  "var VOID=JM([[[],[]]],[['m','f',0]],[[0,[0x10,0]]],[['g',1]]);",
  "var MULTI=JM([[[i32],[i32,i32]]],[['m','f',0]],[[0,[0x20,0,0x10,0]]],[['g',1]]);",
  "var TRAPA=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x20,0,0x10,0,0x1a,0x00]]],[['g',1]]);",
  "var DIVA=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x41,0xe4,0,0x20,0,0x10,0,0x6d]]],[['g',1]]);",
  "var NEST=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x20,0,0x10,0,0x41,1,0x6a]],[0,[0x20,0,0x10,1,0x41,2,0x6c]]],[['g',1],['h',2]]);",
  "var MEMM=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x41,0,0x20,0,0x36,2,0,0x20,0,0x10,0,0x41,0,0x28,2,0,0x6a]]],[['g',1],['mem',0,2]],sec(5,[1,0,1]));",
  "var GLBM=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x20,0,0x24,0,0x20,0,0x10,0,0x23,0,0x6a]]],[['g',1],['gl',0,3]],sec(6,[1,0x7f,1,0x41,0,0x0b]));",
  "var EHM=JM([[[i32],[i32]]],[['m','f',0]],[[0,[0x06,0x7f,0x20,0,0x10,0,0x19,0x41,0x7f,0x0b]]],[['g',1]]);",
  "var AD2=JM([[[i32],[i32]]],[['m','f',0],['m','h',0]],[[0,[0x20,0,0x10,0,0x20,0,0x10,1,0x6a]]],[['g',2]]);",
  "var STARTM=JM([[[i32],[i32]],[[],[]]],[['m','f',0]],[[0,[0x20,0,0x10,0]],[1,[0x41,1,0x10,0,0x1a]]],[['g',1]],[],2);",
].join("");
const jp = (body) => JSPI_PRELUDE + body;
const q = (e, arg, label) => "Q(PR(" + e + ")(" + arg + "), '" + label + "')";
add(
  // Valor devolvido pela importação Suspending e sua conversão ao tipo de retorno.
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); " + q("e.g", 5, "res")),
  jp("var e = X(IMP, S(function(x) { return Promise.reject(new RangeError('r' + x)) })); " + q("e.g", 5, "rej")),
  jp("var e = X(IMP, S(function(x) { return x * 3 })); " + q("e.g", 5, "plain")),
  jp("var e = X(IMP, S(function(x) { })); " + q("e.g", 5, "undef")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve() })); " + q("e.g", 5, "pundef")),
  jp("var e = X(IMP, S(function(x) { return '12' })); " + q("e.g", 5, "str")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve('0x10') })); " + q("e.g", 5, "pstr")),
  jp("var e = X(IMP, S(function(x) { return { valueOf: function() { L('vo'); return 8 } } })); " + q("e.g", 5, "vo")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve({ valueOf: function() { L('vo'); return 8 } }) })); " + q("e.g", 5, "pvo")),
  jp("var e = X(IMP, S(function(x) { return null })); " + q("e.g", 5, "null")),
  jp("var e = X(IMP, S(function(x) { return NaN })); " + q("e.g", 5, "nan")),
  jp("var e = X(IMP, S(function(x) { return 3.9 })); " + q("e.g", 5, "frac")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(4294967297) })); " + q("e.g", 5, "wrap")),
  jp("var e = X(IMP, S(function(x) { return 10n })); " + q("e.g", 5, "big")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(10n) })); " + q("e.g", 5, "pbig")),
  jp("var e = X(IMP, S(function(x) { return Symbol('s') })); " + q("e.g", 5, "sym")),
  jp("var e = X(IMP, S(function(x) { return { then: function(r) { r(9) } } })); " + q("e.g", 5, "thenable")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve({ then: function(r) { r(9) } }) })); " + q("e.g", 5, "pthenable")),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(Promise.resolve(x + 20)) })); " + q("e.g", 5, "nested")),
  jp("var e = X(IMP, S(async function(x) { var a = await Promise.resolve(x); return a + 100 })); " + q("e.g", 5, "async")),
  jp("var e = X(IMP, S(async function(x) { await null; throw new TypeError('late') })); " + q("e.g", 5, "asyncthrow")),
  jp("var e = X(IMP, S(function(x) { throw new RangeError('sync') })); " + q("e.g", 5, "syncthrow")),
  jp("var e = X(IMP, S(function(x) { throw 42 })); " + q("e.g", 5, "throwprim")),
  jp("var o = { k: 1 }; var e = X(IMP, S(function(x) { return Promise.reject(o) })); PR(e.g)(1).catch(function(r) { L(r === o) })"),
  jp("var e = X(IMP, S(function(x) { L(this === undefined); L(arguments.length); L(x); return 1 })); " + q("e.g", -7, "args")),
  jp("var e = X(IMP, S(function(x) { L(x); return 1 })); " + q("e.g", "'4294967295'", "u32")),
  jp("var e = X(IMP, S(function(x) { L(x); return 1 })); " + q("e.g", 2.7, "f")),
  jp("var e = X(IMP, S(function(x) { L(x); return 1 })); " + q("e.g", 1n, "bigarg")),
  jp("var e = X(IMP, S(function(x) { L(x); return 1 })); " + q("e.g", "", "noarg")),
  jp("var e = X(IMP, S(class A {})); " + q("e.g", 1, "class")),
  jp("var e = X(IMP, S(new Proxy(function(x) { return x + 1 }, {}))); " + q("e.g", 1, "proxy")),
  jp("var e = X(IMP, S(function(x) { return x + 1 }.bind(null))); " + q("e.g", 1, "bound")),
  jp("var e = X(IMP, S(WebAssembly.promising(X(ADD).add))); " + q("e.g", 1, "promisingimport")),
  // Ordem entre síncrono, importação, microtarefas e conclusão.
  jp("var e = X(IMP, S(function(x) { L('imp' + x); return Promise.resolve(x) })); L('a'); var p = PR(e.g)(1); L('b'); p.then(function() { L('p') }); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }).then(function() { L('m4') }); L('c')"),
  jp("var e = X(IMP, S(function(x) { L('imp' + x); return x })); L('a'); var p = PR(e.g)(1); L('b'); p.then(function() { L('p') }); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }); L('c')"),
  jp("var e = X(IMP, S(async function(x) { L('imp' + x); return x })); L('a'); var p = PR(e.g)(1); L('b'); p.then(function() { L('p') }); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }).then(function() { L('m4') }).then(function() { L('m5') }); L('c')"),
  jp("var e = X(IMP, S(async function(x) { L('imp' + x); await null; await null; return x })); L('a'); var p = PR(e.g)(1); L('b'); p.then(function() { L('p') }); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }).then(function() { L('m4') }).then(function() { L('m5') }).then(function() { L('m6') }); L('c')"),
  jp("var e = X(IMP, S(function(x) { return Promise.reject(1) })); L('a'); var p = PR(e.g)(1); L('b'); p.catch(function() { L('p') }); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }); L('c')"),
  jp("var e = X(IMP, function(x) { L('imp' + x); return x }); L('a'); var p = PR(e.g)(1); L('b'); p.then(function() { L('p') }); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }); L('c')"),
  jp("var e = X(IMP, S(function(x) { L('imp' + x); return Promise.resolve(x) })); var p1 = PR(e.g)(1); var p2 = PR(e.g)(2); p1.then(function() { L('p1') }); p2.then(function() { L('p2') }); L('sync')"),
  jp("var e = X(IMP, S(function(x) { L('imp' + x); return x })); var p1 = PR(e.g)(1); var p2 = PR(e.g)(2); p1.then(function() { L('p1') }); p2.then(function() { L('p2') }); L('sync')"),
  jp("var e = X(TWO, S(function(x) { L('imp' + x); return Promise.resolve(x + 1) })); var p = PR(e.g)(1); L('after'); " + "Q(p, 'two')"),
  jp("var e = X(TWO, S(function(x) { L('imp' + x); return x + 1 })); var p = PR(e.g)(1); L('after'); Q(p, 'two')"),
  jp("var e = X(TWO, S(function(x) { L('imp' + x); return Promise.resolve(x + 1) })); var p = PR(e.g)(1); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }).then(function() { L('m4') }).then(function() { L('m5') }); Q(p, 'two')"),
  jp("var e = X(TWO, S(function(x) { L('imp' + x); return Promise.reject(new Error('e' + x)) })); L('s'); Q(PR(e.g)(1), 'two')"),
  jp("var e = X(AD2, S(function(x) { L('susp' + x); return Promise.resolve(x * 10) }), function(x) { L('plain' + x); return x + 1 }); L('s'); Q(PR(e.g)(3), 'mixed')"),
  jp("var e = X(AD2, function(x) { L('plain' + x); return x + 1 }, S(function(x) { L('susp' + x); return Promise.resolve(x * 10) })); L('s'); Q(PR(e.g)(3), 'mixed')"),
  jp("var e = X(AD2, S(function(x) { return Promise.resolve(x * 10) }), S(function(x) { return Promise.resolve(x + 1) })); Q(PR(e.g)(3), 'both')"),
  jp("var s = S(function(x) { return Promise.resolve(x * 10) }); var e = X(AD2, s, s); Q(PR(e.g)(3), 'shared')"),
  jp("var s = S(function(x) { return Promise.resolve(x * 10) }); var e1 = X(IMP, s), e2 = X(TWO, s); Q(PR(e1.g)(2), 'a'); Q(PR(e2.g)(2), 'b')"),
  // Recursão wasm e retomada em quadros aninhados.
  jp("var e = X(REC, S(function(x) { return Promise.resolve(x + 1) })); Q(PR(e.g)(0), 'rec0'); Q(PR(e.g)(1), 'rec1'); Q(PR(e.g)(5), 'rec5')"),
  jp("var e = X(REC, S(function(x) { return x + 1 })); Q(PR(e.g)(0), 'rec0'); Q(PR(e.g)(4), 'rec4')"),
  jp("var e = X(REC, S(function(x) { return x % 2 ? Promise.resolve(x) : x })); Q(PR(e.g)(9), 'mixed')"),
  jp("var e = X(REC, S(async function(x) { await null; return x })); Q(PR(e.g)(30), 'rec30')"),
  jp("var e = X(REC, S(function(x) { return Promise.resolve(1) })); Q(PR(e.g)(200), 'rec200')"),
  jp("var e = X(REC, S(function(x) { L('f' + x); if (x === 2) return Promise.reject(new Error('at2')); return Promise.resolve(x) })); Q(PR(e.g)(4), 'rec')"),
  jp("var e = X(REC, S(function(x) { L('f' + x); return Promise.resolve(x) })); Q(PR(e.g)(3), 'rec')"),
  jp("var e = X(NEST, S(function(x) { return Promise.resolve(x + 10) })); Q(PR(e.g)(1), 'g'); Q(PR(e.h)(1), 'h')"),
  jp("var e = X(NEST, S(function(x) { return x + 10 })); Q(PR(e.h)(1), 'h')"),
  jp("var e = X(NEST, S(function(x) { return Promise.reject(new SyntaxError('n')) })); Q(PR(e.h)(1), 'h'); Q(PR(e.g)(1), 'g')"),
  // Promising dentro de promising e Suspending que chama promising.
  jp("var inner = X(IMP, function(x) { return x * 2 }); var e = X(IMP, S(function(x) { return PR(inner.g)(x) })); Q(PR(e.g)(21), 'nested')"),
  jp("var inner = X(IMP, S(function(x) { return Promise.resolve(x * 2) })); var e = X(IMP, S(function(x) { return PR(inner.g)(x) })); Q(PR(e.g)(21), 'nested')"),
  jp("var inner = X(IMP, S(function(x) { return Promise.reject(new Error('inner' + x)) })); var e = X(IMP, S(function(x) { return PR(inner.g)(x) })); Q(PR(e.g)(1), 'nested')"),
  jp("var e = X(IMP, S(function(x) { return x > 0 ? PR(e.g)(x - 1).then(function(v) { return v + x }) : 0 })); Q(PR(e.g)(10), 'tri')"),
  jp("var e = X(IMP, S(function(x) { L('in' + x); return x > 0 ? PR(e.g)(x - 1).then(function(v) { L('out' + x); return v + x }) : 0 })); Q(PR(e.g)(3), 'tri')"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); var p = PR(e.g); Q(p(1), 'a'); Q(p(2), 'b'); Q(p(3), 'c')"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); var p = PR(e.g); var q1 = p(0); for (var i = 0; i < 100; i++) q1 = q1.then(function(v) { return p(v) }); Q(q1, 'chain')"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); var p = PR(e.g); var all = []; for (var i = 0; i < 20; i++) all.push(p(i)); Promise.all(all).then(function(v) { L(v.join(',')) })"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); (async function() { var v = await PR(e.g)(3); L('v' + v); var w = await PR(e.g)(v); L('w' + w) })()"),
  jp("var e = X(IMP, S(function(x) { return Promise.reject(new Error('bad')) })); (async function() { try { await PR(e.g)(3) } catch (err) { L('caught:' + E(err)) } L('after') })()"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); L(T(function() { return new (PR(e.g))(1) })); L(T(function() { return WebAssembly.promising(PR(e.g)) }))"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); var p = PR(e.g); Q(p.call({}, 1), 'call'); Q(p.apply(null, [2]), 'apply'); Q(Reflect.apply(p, undefined, [3]), 'reflect'); Q(p.bind(null, 4)(), 'bind')"),
  // Pendente assentada por funções capturadas: ordem de retomada e promessa nunca assentada.
  jp("var rs = []; var e = X(IMP, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(1), 'p1'); Q(PR(e.g)(2), 'p2'); L('pending' + rs.length); rs[1](20); rs[0](10)"),
  jp("var rs = []; var e = X(IMP, S(function(x) { return new Promise(function(r, j) { rs.push([r, j]) }) })); Q(PR(e.g)(1), 'p1'); Q(PR(e.g)(2), 'p2'); rs[0][1](new Error('no')); rs[1][0](7)"),
  jp("var rs = []; var e = X(IMP, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(1), 'never'); L('only sync')"),
  jp("var rs = []; var e = X(IMP, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(1), 'late'); Promise.resolve().then(function() { L('m1'); rs[0](5) }).then(function() { L('m2') }).then(function() { L('m3') })"),
  jp("var rs = []; var e = X(IMP, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(1), 'late'); rs[0](Promise.resolve(6))"),
  jp("var rs = []; var e = X(TWO, S(function(x) { L('f' + x); return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(1), 'two'); L('n' + rs.length); rs[0](10); Promise.resolve().then(function() { return null }).then(function() { return null }).then(function() { L('n' + rs.length); rs[1](20) })"),
  jp("var e = X(IMP, S(function(x) { var p = Promise.resolve(x); p.then = function(r) { L('then'); r(77) }; return p })); Q(PR(e.g)(1), 'ownthen')"),
  jp("class P2 extends Promise {} ; var e = X(IMP, S(function(x) { return P2.resolve(x + 1) })); Q(PR(e.g)(1), 'sub')"),
  jp("var e = X(IMP, S(function(x) { var p = Promise.resolve(x + 1); p.constructor = function() { throw new Error('ctor') }; return p })); Q(PR(e.g)(1), 'ctor')"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x).then().then().then(function(v) { return v + 1 }) })); Q(PR(e.g)(1), 'hops'); Promise.resolve().then(function() { L('m1') }).then(function() { L('m2') }).then(function() { L('m3') }).then(function() { L('m4') }).then(function() { L('m5') })"),
  // Tipos de parâmetro e resultado.
  jp("var e = X(PT(i64), S(function(x) { L(typeof x); return Promise.resolve(x + 1n) })); Q(PR(e.g)(5n), 'i64')"),
  jp("var e = X(PT(i64), S(function(x) { return Promise.resolve(1) })); Q(PR(e.g)(5n), 'i64num')"),
  jp("var e = X(PT(i64), S(function(x) { return x })); Q(PR(e.g)(-5n), 'i64neg'); Q(PR(e.g)(5), 'i64arg')"),
  jp("var e = X(PT(f64), S(function(x) { return Promise.resolve(x / 2) })); Q(PR(e.g)(5), 'f64'); Q(PR(e.g)(NaN), 'nan'); Q(PR(e.g)(Infinity), 'inf')"),
  jp("var e = X(PT(f32), S(function(x) { L(x); return Promise.resolve(x) })); Q(PR(e.g)(0.1), 'f32')"),
  jp("var e = X(PT(f64), S(function(x) { return '2.5' })); Q(PR(e.g)(1), 'f64str'); Q(PR(e.g)(-0), 'f64neg0')"),
  jp("var o = { a: 1 }; var e = X(PT(xr), S(function(x) { L(x === o); return Promise.resolve(x) })); PR(e.g)(o).then(function(v) { L(v === o) })"),
  jp("var e = X(PT(xr), S(function(x) { return Promise.resolve(undefined) })); Q(PR(e.g)(1), 'xr')"),
  jp("var e = X(PT(xr), S(function(x) { return Promise.resolve(null) })); Q(PR(e.g)(1), 'xrnull')"),
  jp("var e = X(PT(xr), S(function(x) { return 'str' })); Q(PR(e.g)(1), 'xrstr')"),
  jp("var e = X(PT(xr), S(function(x) { return Promise.resolve(Symbol.iterator) })); PR(e.g)(1).then(function(v) { L(v === Symbol.iterator) })"),
  // Importação sem resultado e com resultados múltiplos.
  jp("var e = X(VOID, S(function() { L('f'); return Promise.resolve(5) })); Q(PR(e.g)(), 'void')"),
  jp("var e = X(VOID, S(function() { L('f'); return Promise.reject(new Error('v')) })); Q(PR(e.g)(), 'voidrej')"),
  jp("var e = X(VOID, S(function() { L('f'); return 1 })); Q(PR(e.g)(), 'voidplain'); L('s')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.resolve([x, x + 1]) })); Q(PR(e.g)(1), 'multi')"),
  jp("var e = X(MULTI, S(function(x) { return [x, x + 1] })); Q(PR(e.g)(1), 'multiplain')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.resolve([x]) })); Q(PR(e.g)(1), 'short')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.resolve([1, 2, 3]) })); Q(PR(e.g)(1), 'long')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.resolve(5) })); Q(PR(e.g)(1), 'noniter')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.resolve(new Set([7, 8])) })); Q(PR(e.g)(1), 'set')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.resolve('ab') })); Q(PR(e.g)(1), 'strmulti')"),
  jp("var e = X(MULTI, S(function(x) { return Promise.reject(new Error('m')) })); Q(PR(e.g)(1), 'multirej')"),
  jp("var e = X(MULTI, function(x) { return [x, 2] }); L(T(function() { return e.g(1) })); Q(PR(e.g)(1), 'nosusp')"),
  // Exceções e traps do wasm depois de retomar.
  jp("var e = X(DIVA, S(function(x) { return Promise.resolve(x) })); Q(PR(e.g)(0), 'zero'); Q(PR(e.g)(5), 'five')"),
  jp("var e = X(DIVA, S(function(x) { return Promise.resolve(0) })); PR(e.g)(1).catch(function(err) { L(err instanceof WebAssembly.RuntimeError); L(err.name); L(err.message) })"),
  jp("var e = X(DIVA, S(function(x) { return 0 })); Q(PR(e.g)(1), 'plainzero'); Q(PR(e.g)(1), 'again')"),
  jp("var e = X(TRAPA, S(function(x) { return Promise.resolve(x) })); Q(PR(e.g)(1), 'unreach'); Q(PR(e.g)(2), 'unreach2')"),
  jp("var e = X(TRAPA, S(function(x) { return x })); Q(PR(e.g)(1), 'unreach')"),
  jp("var e = X(NEST, S(function(x) { return Promise.resolve(0) })); Q(PR(e.h)(1), 'nest'); Q(PR(e.g)(1), 'nestg')"),
  jp("var n = 0; var e = X(DIVA, S(function(x) { return Promise.resolve(n++ % 2) })); Q(PR(e.g)(1), 'a'); Q(PR(e.g)(1), 'b'); Q(PR(e.g)(1), 'c'); Q(PR(e.g)(1), 'd')"),
  jp("var e = X(EHM, S(function(x) { return Promise.resolve(x + 1) })); Q(PR(e.g)(1), 'eh')"),
  jp("var e = X(EHM, S(function(x) { return Promise.reject(new Error('r')) })); Q(PR(e.g)(1), 'ehrej')"),
  jp("var e = X(EHM, S(function(x) { throw new Error('s') })); Q(PR(e.g)(1), 'ehsync')"),
  jp("var e = X(EHM, S(function(x) { return x + 1 })); Q(PR(e.g)(1), 'ehplain')"),
  jp("var e = X(EHM, S(function(x) { return x + 1 })); L(T(function() { return e.g(1) }))"),
  jp("var e = X(EHM, S(function(x) { return Promise.reject(7) })); Q(PR(e.g)(1), 'ehprim'); Q(PR(e.g)(2), 'ehprim2')"),
  // Estado da instância durante a suspensão.
  jp("var e = X(MEMM, S(function(x) { return Promise.resolve(x) })); Q(PR(e.g)(5), 'mem'); L(new Uint32Array(e.mem.buffer)[0])"),
  jp("var e; e = X(MEMM, S(function(x) { new Uint32Array(e.mem.buffer)[0] = 100; return Promise.resolve(x) })); Q(PR(e.g)(5), 'memjs')"),
  jp("var e; var b; e = X(MEMM, S(function(x) { b = e.mem.buffer; e.mem.grow(1); return Promise.resolve(x) })); Q(PR(e.g)(5), 'grow'); L(b.byteLength); L(e.mem.buffer.byteLength)"),
  jp("var rs = []; var e = X(MEMM, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(5), 'a'); Q(PR(e.g)(6), 'b'); rs[0](1); rs[1](1); Promise.resolve().then(function() { return null }).then(function() { L(new Uint32Array(e.mem.buffer)[0]) })"),
  jp("var e = X(GLBM, S(function(x) { return Promise.resolve(x) })); Q(PR(e.g)(5), 'glb'); L(e.gl.value)"),
  jp("var rs = []; var e = X(GLBM, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(5), 'a'); Q(PR(e.g)(7), 'b'); L(e.gl.value); rs[0](1); rs[1](1)"),
  jp("var rs = []; var e = X(GLBM, S(function(x) { e.gl.value = 1000; return new Promise(function(r) { rs.push(r) }) })); Q(PR(e.g)(5), 'a'); rs[0](1)"),
  // SuspendError: fora de promising, frames JS no meio, Suspending chamado de JS.
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); try { e.g(1) } catch (err) { L(err instanceof WebAssembly.SuspendError); L(err instanceof Error); L(err.name); L(err.message) }"),
  jp("var e = X(IMP, S(function(x) { return x })); L(T(function() { return e.g(1) }))"),
  jp("var e = X(IMP, S(function(x) { L('called'); return x })); L(T(function() { return e.g(1) }))"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); L(T(function() { return e.g(1) })); Q(PR(e.g)(1), 'after')"),
  jp("var e = X(TWO, S(function(x) { L('f' + x); return Promise.resolve(x) })); L(T(function() { return e.g(1) }))"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); L(T(function() { return Reflect.apply(e.g, null, [1]) })); L(T(function() { return e.g.call(null, 1) }))"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); [1, 2].forEach(function(v) { L(T(function() { return e.g(v) })) })"),
  jp("var inner = X(IMP, S(function(x) { return Promise.resolve(x) })); var e = X(IMP, function(x) { return inner.g(x) }); Q(PR(e.g)(1), 'frames')"),
  jp("var inner = X(IMP, S(function(x) { return x })); var e = X(IMP, function(x) { return inner.g(x) }); Q(PR(e.g)(1), 'framesplain')"),
  jp("var inner = X(IMP, S(function(x) { L('inner'); return Promise.resolve(x) })); var e = X(IMP, function(x) { try { return inner.g(x) } catch (err) { L('caught:' + E(err)); return -1 } }); Q(PR(e.g)(1), 'framescaught')"),
  jp("var inner = X(IMP, S(function(x) { return Promise.resolve(x) })); var e = X(EHM, function(x) { return inner.g(x) }); Q(PR(e.g)(1), 'framesEH')"),
  jp("var inner = X(IMP, S(function(x) { return Promise.resolve(x) })); var e = X(IMP, inner.g); Q(PR(e.g)(1), 'direct'); Q(PR(inner.g)(2), 'inner')"),
  jp("var inner = X(IMP, function(x) { return x + 1 }); var e = X(IMP, inner.g); Q(PR(e.g)(1), 'directplain')"),
  jp("var inner = X(IMP, function(x) { return x + 1 }); var e = X(IMP, function(x) { return PR(inner.g)(x) }); Q(PR(e.g)(1), 'promisenotsusp')"),
  jp("var e = X(IMP, function(x) { return x }); L(T(function() { return new WebAssembly.Suspending(e.g)(1) }))"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); var f = e.g; var w = PR(f); L(T(function() { return f(1) })); Q(w(1), 'w')"),
  jp("L(T(function() { return new WebAssembly.Instance(new WebAssembly.Module(STARTM), { m: { f: S(function(x) { return Promise.resolve(x) }) } }) }))"),
  jp("L(T(function() { return new WebAssembly.Instance(new WebAssembly.Module(STARTM), { m: { f: function(x) { L('start' + x); return x } } }) }))"),
  jp("Q(WebAssembly.instantiate(STARTM, { m: { f: S(function(x) { return Promise.resolve(x) }) } }), 'inst')"),
  jp("var rs = []; var e = X(IMP, S(function(x) { return new Promise(function(r) { rs.push(r) }) })); var p = PR(e.g)(1); L(T(function() { return e.g(2) })); rs[0](3); Q(p, 'pending')"),
  // promising com funções comuns e argumentos.
  jp("var e = X(ADD); Q(PR(e.add)(2, 3), 'add'); Q(PR(e.add)(), 'noargs'); Q(PR(e.add)('7', 1.9), 'coerce')"),
  jp("var e = X(ADD); Q(PR(e.add)(1n, 2), 'bigint'); L('sync')"),
  jp("var e = X(ADD); L(T(function() { return PR(e.add)(Symbol(), 1) })); L('sync')"),
  jp("var e = X(ADD); var p = PR(e.add)(2, 3); L(p instanceof Promise); L(Object.getPrototypeOf(p) === Promise.prototype); L(p.constructor === Promise)"),
  jp("var e = X(ADD); Q(PR(e.add)({ valueOf: function() { L('vo'); return 4 } }, 1), 'valueOf'); L('sync')"),
  jp("var e = X(DIV); Q(PR(e.d)(), 'div'); Q(PR(e.d)(), 'div2'); L('sync')"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); var p = PR(e.g); L(T(function() { return p.length + ':' + p.name })); L(typeof p.prototype); L(Object.getPrototypeOf(p) === Function.prototype)"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x) })); L(PR(e.g) === PR(e.g)); L(PR(e.g) === e.g)"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); var p = Promise.all([PR(e.g)(1), PR(e.g)(2)]); p.then(function(v) { L(v.join()) }); Promise.race([PR(e.g)(5), PR(e.g)(6)]).then(function(v) { L('race' + v) })"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); PR(e.g)(1).finally(function() { L('fin') }).then(function(v) { L('v' + v) })"),
  jp("var e = X(IMP, S(function(x) { return Promise.resolve(x + 1) })); var p = PR(e.g)(1); p.then(function(v) { L('a' + v) }); p.then(function(v) { L('b' + v) }); L(p === PR(e.g)(1))")
);
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-js-golden-"));
const lines = [];
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (let i = programs.length - 1; i >= 0; i--) if (HOST.test(programs[i])) programs.splice(i, 1);
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
