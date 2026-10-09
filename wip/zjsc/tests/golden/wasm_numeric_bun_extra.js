// Auxiliar dos programas de instruções numéricas do WebAssembly, usado por scripts/gen-wasm-numeric-golden.js e por
// tests/wasm_numeric_bun_golden.rs depois de wasm_js_bun_harness.js, wasm_exceptions_bun_extra.js (RUN, C, X, E, L, T)
// e wasm_gc_bun_extra.js (HX).
// Z chama f e descreve o resultado (zero negativo como "-0", BigInt com sufixo "n") ou a exceção lançada.
globalThis.FM = function (v) {
  if (typeof v === "bigint") return v + "n";
  if (typeof v === "number" && Object.is(v, -0)) return "-0";
  return String(v);
};
globalThis.Z = function (f) {
  try { return "ok:" + FM(f()); } catch (e) { return "throws " + X(e); }
};
// Tipos de lane de v128: tamanho em bytes e construtor do array tipado.
globalThis.LT = {
  i8: [1, Int8Array], i16: [2, Int16Array], i32: [4, Int32Array], i64: [8, BigInt64Array],
  f32: [4, Float32Array], f64: [8, Float64Array],
};
globalThis.W = function (x, off, type, lanes) {
  new LT[type][1](x.mem.buffer, off, lanes.length).set(lanes);
};
// Lê as lanes de um v128 na memória; NaN de ponto flutuante sai com os bits, para observar a canonicalização.
globalThis.R = function (x, off, type) {
  var size = LT[type][0];
  var lanes = new LT[type][1](x.mem.buffer, off, 16 / size);
  var out = [];
  for (var i = 0; i < lanes.length; i++) {
    var v = lanes[i];
    if (typeof v === "number" && v !== v) {
      var raw = new Uint8Array(x.mem.buffer, off + i * size, size), hex = "";
      for (var j = size - 1; j >= 0; j--) hex += (raw[j] < 16 ? "0" : "") + raw[j].toString(16);
      out.push("NaN:0x" + hex);
    } else out.push(FM(v));
  }
  return out.join(",");
};
// Escreve os vetores `vecs` (16 bytes cada, a partir de 0), chama `name` e devolve o v128 em 64 (ou o escalar se ot = "s").
globalThis.SV = function (x, name, it, ot, vecs, args) {
  try {
    for (var i = 0; i < vecs.length; i++) W(x, i * 16, it, vecs[i]);
    var r = x[name].apply(null, args || []);
    return "ok:" + (ot === "s" ? FM(r) : R(x, 64, ot));
  } catch (e) { return "throws " + X(e); }
};
// Bytes da memória de `from` até `to`, separados por vírgula.
globalThis.MB = function (x, from, to) {
  return Array.prototype.join.call(new Uint8Array(x.mem.buffer).slice(from, to), ",");
};
