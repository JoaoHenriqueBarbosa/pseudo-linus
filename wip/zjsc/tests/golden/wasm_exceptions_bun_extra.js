// Auxiliares e módulos binários de exceções do WebAssembly (seção Tag id 13, try/catch/throw), usados por
// scripts/gen-wasm-exceptions-golden.js e tests/wasm_exceptions_bun_golden.rs depois de wasm_js_bun_harness.js.
var I32 = 0x7f, I64 = 0x7e, F32 = 0x7d, F64 = 0x7c;
globalThis.vec = function (items) {
  return [items.length].concat(Array.prototype.concat.apply([], items));
};
// Seção com o tamanho em LEB128 (a `sec` do harness base só cabe em um byte).
globalThis.lsec = function (id, payload) {
  var size = [], n = payload.length;
  do { var b = n & 0x7f; n >>>= 7; size.push(n ? b | 0x80 : b); } while (n);
  return [id].concat(size, payload);
};
// Monta um módulo: types [[params, results]], imports [[módulo, campo, bytes do tipo]], funcs (índices de
// tipo), tags (índices de tipo), exports [[nome, tipo, índice]], codes (corpos sem locais e sem o `end`).
globalThis.EXM = function (o) {
  var types = o.types || [], imports = o.imports || [], funcs = o.funcs || [], tags = o.tags || [];
  var exports = o.exports || [], codes = o.codes || [];
  var secs = [];
  secs.push(lsec(1, vec(types.map(function (t) { return [0x60, t[0].length].concat(t[0], [t[1].length], t[1]); }))));
  if (imports.length) secs.push(lsec(2, vec(imports.map(function (i) { return str(i[0]).concat(str(i[1]), i[2]); }))));
  secs.push(lsec(3, vec(funcs.map(function (f) { return [f]; }))));
  if (tags.length) secs.push(lsec(13, vec(tags.map(function (t) { return [0, t]; }))));
  secs.push(lsec(7, vec(exports.map(function (x) { return str(x[0]).concat([x[1], x[2]]); }))));
  secs.push(lsec(10, vec(codes.map(function (c) { var body = [0].concat(c, [0x0b]); return [body.length].concat(body); }))));
  return mk.apply(null, secs);
};
// Instancia os bytes e devolve os exports.
globalThis.RUN = function (bytes, imports) {
  return new WebAssembly.Instance(new WebAssembly.Module(bytes), imports).exports;
};
// Descreve uma exceção capturada: o que o programa precisa comparar com o bun.
globalThis.X = function (e) {
  return (e instanceof WebAssembly.Exception ? "Exception" : e instanceof Error ? E(e) : typeof e + ":" + String(e));
};
// Chama f e descreve o resultado ou a exceção lançada.
globalThis.C = function (f) {
  try { return "ok:" + String(f()); } catch (e) { return "throws " + X(e); }
};
// Tipos de EHA: 0 (i32)->(), 1 ()->(), 2 (i32)->i32, 3 (i32,f64)->(), 4 ()->i32. Tags: 0 (i32), 1 (), 2 (i32,f64).
globalThis.EHA = EXM({
  types: [[[I32], []], [[], []], [[I32], [I32]], [[I32, F64], []], [[], [I32]]],
  funcs: [0, 1, 3, 2, 2, 2, 4, 2, 2, 2, 2, 3, 2, 4],
  tags: [0, 1, 3],
  exports: [
    ["thr0", 0, 0], ["thr1", 0, 1], ["thr2", 0, 2], ["catch0", 0, 3], ["catchAll", 0, 4], ["onlyTag1", 0, 5],
    ["call1", 0, 6], ["rethrow", 0, 7], ["delegate", 0, 8], ["nested", 0, 9], ["catchThrow", 0, 10],
    ["catch2", 0, 11], ["through", 0, 12], ["trapCatch", 0, 13], ["t0", 4, 0], ["t1", 4, 1], ["t2", 4, 2]
  ],
  codes: [
    [0x20, 0, 0x08, 0],
    [0x08, 1],
    [0x20, 0, 0x20, 1, 0x08, 2],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 0, 0x0b],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x19, 0x41, 59, 0x0b],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 1, 0x41, 55, 0x0b],
    [0x06, 0x7f, 0x10, 1, 0x41, 0, 0x07, 1, 0x41, 9, 0x0b],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 0, 0x1a, 0x09, 0, 0x0b],
    [0x06, 0x7f, 0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x18, 0, 0x07, 0, 0x41, 0x32, 0x6a, 0x0b],
    [0x06, 0x7f, 0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 1, 0x41, 1, 0x0b, 0x07, 0, 0x41, 0x28, 0x6a, 0x0b],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 0, 0x1a, 0x08, 1, 0x0b],
    [0x06, 0x40, 0x20, 0, 0x20, 1, 0x10, 2, 0x07, 2, 0x1a, 0x1a, 0x0b],
    [0x20, 0, 0x10, 0, 0x41, 0],
    [0x06, 0x7f, 0x00, 0x41, 0, 0x19, 0x41, 1, 0x0b]
  ]
});
// Importa a tag t (i32) e a função f ((i32)->()). Funções: 1 catchImp, 2 catchAllImp, 3 thr, 4 rethrowAll.
globalThis.EHI = EXM({
  types: [[[I32], []], [[I32], [I32]]],
  imports: [["m", "t", [4, 0, 0]], ["m", "f", [0, 0]]],
  funcs: [1, 1, 0, 1],
  exports: [["catchImp", 0, 1], ["catchAllImp", 0, 2], ["thr", 0, 3], ["rethrowAll", 0, 4], ["t", 4, 0]],
  codes: [
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x07, 0, 0x0b],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x19, 0x41, 59, 0x0b],
    [0x20, 0, 0x08, 0],
    [0x06, 0x7f, 0x20, 0, 0x10, 0, 0x41, 0, 0x19, 0x09, 0, 0x0b]
  ]
});
// Uma tag exportada sem uso e uma função que lança com um payload i64 e f32 (tag 0: (i64), tag 1: (f32)).
globalThis.EHN = EXM({
  types: [[[I64], []], [[F32], []]],
  funcs: [0, 1],
  tags: [0, 1],
  exports: [["thr64", 0, 0], ["thr32", 0, 1], ["t64", 4, 0], ["t32", 4, 1]],
  codes: [[0x20, 0, 0x08, 0], [0x20, 0, 0x08, 1]]
});
// try_table (tag 0: (i32)): 0 thr0, 1 catchTT, 2 catchAllTT, 3 throwRef.
globalThis.EHT = EXM({
  types: [[[I32], []], [[I32], [I32]]],
  funcs: [0, 1, 1, 1],
  tags: [0],
  exports: [["thr0", 0, 0], ["catchTT", 0, 1], ["catchAllTT", 0, 2], ["throwRef", 0, 3], ["t0", 4, 0]],
  codes: [
    [0x20, 0, 0x08, 0],
    [0x02, 0x7f, 0x1f, 0x40, 1, 0x00, 0x00, 0x00, 0x20, 0, 0x10, 0, 0x0b, 0x41, 0, 0x0b],
    [0x02, 0x40, 0x1f, 0x40, 1, 0x02, 0x00, 0x20, 0, 0x10, 0, 0x0b, 0x0b, 0x41, 0x28],
    [0x02, 0x69, 0x1f, 0x40, 1, 0x03, 0x00, 0x20, 0, 0x10, 0, 0x0b, 0xd0, 0x69, 0x0b, 0x0a]
  ]
});
