// Auxiliar do golden de SIMD: vem depois de wasm_js_bun_harness.js (usa `FN`). `S(paramCount, body, args)` monta um
// módulo de uma função "f" com `paramCount` parâmetros i64 e dois resultados i64 (multi-value), com o corpo `body`
// (bytes, sem o `end`) e uma memória de uma página; chama `f(...args)` (BigInt) e devolve o array de BigInt em texto.
globalThis.S = function (paramCount, body, args) {
  var params = [];
  for (var i = 0; i < paramCount; i++) params.push(0x7e);
  var instance = new WebAssembly.Instance(new WebAssembly.Module(FN(params, [0x7e, 0x7e], body)));
  return String(instance.exports.f.apply(null, args));
};
