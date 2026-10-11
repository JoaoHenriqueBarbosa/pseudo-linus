// Auxiliares dos programas de recent_apis_bun.tsv: S serializa um valor, P registra o desfecho de uma promise.
// O gerador e tests/recent_apis_bun_golden.rs embutem este mesmo texto depois de async_bun_harness.js.
globalThis.S = function (v, d) {
  d = d || 0;
  var t = typeof v;
  if (v === null) return "null";
  if (t === "undefined") return "undefined";
  if (t === "number") return Object.is(v, -0) ? "-0" : String(v);
  if (t === "string") return JSON.stringify(v);
  if (t === "boolean") return String(v);
  if (t === "bigint") return v + "n";
  if (t === "symbol") return String(v);
  if (t === "function") return "fn:" + v.name + "/" + v.length;
  if (d > 3) return "...";
  if (v instanceof Error) return "E:" + v.constructor.name + ":" + v.message;
  if (ArrayBuffer.isView(v)) return v.constructor.name + "[" + Array.from(v, function (x) { return S(x); }).join(",") + "]";
  if (Array.isArray(v)) return "[" + Array.from(v, function (x) { return S(x, d + 1); }).join(",") + "]";
  if (v instanceof Map) return "Map{" + Array.from(v, function (e) { return S(e[0], d + 1) + "=>" + S(e[1], d + 1); }).join(",") + "}";
  if (v instanceof Set) return "Set{" + Array.from(v, function (x) { return S(x, d + 1); }).join(",") + "}";
  var o = [];
  Reflect.ownKeys(v).forEach(function (k) {
    var x = Object.getOwnPropertyDescriptor(v, k);
    o.push(String(k) + (x.enumerable ? "" : "~") + ":" + ("value" in x ? S(x.value, d + 1) : "(accessor)"));
  });
  return (Object.getPrototypeOf(v) === null ? "N" : "") + "{" + o.join(",") + "}";
};
globalThis.T = function (f) {
  try { L(S(f())); } catch (e) { L("T:" + (e && e.constructor && e.constructor.name) + ":" + (e && e.message)); }
};
globalThis.P = function (p) {
  p.then(function (v) { L("ok:" + S(v)); }, function (e) { L("err:" + S(e)); });
};
