// Auxiliar dos programas de WebAssembly GC e tipos de referência, usado por scripts/gen-wasm-gc-golden.js e por
// tests/wasm_gc_bun_golden.rs depois de wasm_js_bun_harness.js e wasm_exceptions_bun_extra.js (que dão RUN, C, X, E, L, T).
// HX decodifica um módulo em hexadecimal (montado de wat com wasm-tools na hora de gerar o golden).
globalThis.HX = function (hex) {
  var out = new Uint8Array(hex.length / 2);
  for (var i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
};
