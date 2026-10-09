globalThis.log = [];
globalThis.__err = null;
globalThis.L = function (x) { log.push(x); };
globalThis.E = function (e) {
  if (e === null || (typeof e !== "object" && typeof e !== "function")) return "primitive:" + String(e);
  return e.constructor.name + ":" + String(e.message).replace(/ \(evaluating '.*\)$/, "");
};
globalThis.D = function (v) {
  if (v instanceof WebAssembly.Module) return "Module";
  if (v instanceof WebAssembly.Instance) return "Instance:" + Object.keys(v.exports).join(",");
  if (v !== null && typeof v === "object") return "{" + Object.keys(v).join(",") + "}";
  return String(v);
};
globalThis.P = function (p, label) {
  p.then(function (v) { L(label + ":ok:" + D(v)); }, function (e) { L(label + ":err:" + E(e)); });
};
globalThis.T = function (f) {
  try { return String(f()); } catch (e) { return "throws " + E(e); }
};
globalThis.R = function (v) {
  return typeof v === "bigint" ? v + "n" : Object.is(v, -0) ? "-0" : typeof v === "symbol" ? "symbol" : String(v);
};
globalThis.C = function (f) {
  try { return R(f()); } catch (e) { return "throws " + E(e); }
};
globalThis.N = function (o) { return Object.getOwnPropertyNames(o).join(","); };
globalThis.F = function (o, k) {
  var d = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "none";
  return ("value" in d ? "v" + (d.writable ? "w" : "-") : (d.get ? "g" : "-") + (d.set ? "s" : "-")) + (d.enumerable ? "e" : "-") + (d.configurable ? "c" : "-");
};
globalThis.str = function (s) {
  return [s.length].concat(Array.prototype.map.call(s, function (c) { return c.charCodeAt(0); }));
};
globalThis.sec = function (id, payload) { return [id, payload.length].concat(payload); };
globalThis.mk = function () {
  var out = [0, 97, 115, 109, 1, 0, 0, 0];
  for (var i = 0; i < arguments.length; i++) out = out.concat(arguments[i]);
  return new Uint8Array(out);
};
globalThis.mut = function (bytes, index, value) {
  var copy = new Uint8Array(bytes);
  copy[index] = value;
  return copy;
};
var typeAdd = sec(1, [1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f]);
var typeVoid = sec(1, [1, 0x60, 0, 0]);
var typeRetI32 = sec(1, [1, 0x60, 0, 1, 0x7f]);
globalThis.EMPTY = mk();
globalThis.ADD = mk(typeAdd, sec(3, [1, 0]), sec(7, [1].concat(str("add"), [0, 0])), sec(10, [1, 7, 0, 0x20, 0, 0x20, 1, 0x6a, 0x0b]));
globalThis.IMP = mk(
  sec(1, [1, 0x60, 1, 0x7f, 1, 0x7f]),
  sec(2, [1].concat(str("m"), str("f"), [0, 0])),
  sec(3, [1, 0]),
  sec(7, [1].concat(str("g"), [0, 1])),
  sec(10, [1, 6, 0, 0x20, 0, 0x10, 0, 0x0b])
);
globalThis.MEM = mk(sec(5, [1, 0, 1]), sec(7, [1].concat(str("mem"), [2, 0])));
globalThis.TAB = mk(sec(4, [1, 0x70, 0, 2]), sec(7, [1].concat(str("tab"), [1, 0])));
globalThis.GLB = mk(sec(6, [1, 0x7f, 1, 0x41, 42, 0x0b]), sec(7, [1].concat(str("g"), [3, 0])));
globalThis.START = mk(typeVoid, sec(3, [1, 0]), sec(6, [1, 0x7f, 1, 0x41, 0, 0x0b]), sec(7, [1].concat(str("g"), [3, 0])), sec(8, [0]), sec(10, [1, 6, 0, 0x41, 7, 0x24, 0, 0x0b]));
globalThis.IMPMEM = mk(sec(2, [1].concat(str("m"), str("mem"), [2, 0, 1])));
globalThis.IMPTAB = mk(sec(2, [1].concat(str("m"), str("tab"), [1, 0x70, 0, 1])));
globalThis.IMPGLB = mk(sec(2, [1].concat(str("m"), str("g"), [3, 0x7f, 0])));
globalThis.CUSTOM = mk(sec(0, str("a").concat([0xaa])), sec(0, str("bc").concat([7])), sec(0, str("a").concat([1, 2, 3])));
// Módulo com uma função "f" de tipo (params) -> (results), memória "mem" de uma página, tabela "tab" de dois
// elementos de funcref e corpo `body` (sem locais nem `end`).
globalThis.FX = function (params, results, body) {
  var code = [0].concat(body, [0x0b]);
  return mk(
    sec(1, [1, 0x60, params.length].concat(params, [results.length], results)),
    sec(3, [1, 0]),
    sec(4, [1, 0x70, 0, 2]),
    sec(5, [1, 0, 1]),
    sec(7, [3].concat(str("f"), [0, 0], str("mem"), [2, 0], str("tab"), [1, 0])),
    sec(10, [1, code.length].concat(code))
  );
};
globalThis.IDT = function (t) { return FX([t], [t], [0x20, 0]); };
globalThis.TRAP = FX([], [], [0x00]);
// Módulo que importa m.f de tipo (params) -> (results) e a reexporta como "f".
globalThis.REEXP = function (params, results) {
  return mk(
    sec(1, [1, 0x60, params.length].concat(params, [results.length], results)),
    sec(2, [1].concat(str("m"), str("f"), [0, 0])),
    sec(7, [1].concat(str("f"), [0, 0]))
  );
};
// Módulo com global "g" do tipo `t`, mutável ou não, inicializado pelos bytes `init` (sem o `end`).
globalThis.GX = function (t, mutable, init) {
  return mk(sec(6, [1, t, mutable ? 1 : 0].concat(init, [0x0b])), sec(7, [1].concat(str("g"), [3, 0])));
};
globalThis.ALLIMP = mk(
  typeVoid,
  sec(2, [4].concat(str("m"), str("f"), [0, 0], str("m"), str("t"), [1, 0x70, 0, 1], str("m"), str("mem"), [2, 1, 1, 2], str("m"), str("g"), [3, 0x7f, 1]))
);
globalThis.ALLEXP = mk(
  typeVoid,
  sec(3, [1, 0]),
  sec(4, [1, 0x70, 0, 1]),
  sec(5, [1, 0, 1]),
  sec(13, [1, 0, 0]),
  sec(6, [1, 0x7f, 0, 0x41, 1, 0x0b]),
  sec(7, [5].concat(str("f"), [0, 0], str("t"), [1, 0], str("mem"), [2, 0], str("e"), [4, 0], str("g"), [3, 0])),
  sec(10, [1, 2, 0, 0x0b])
);
globalThis.TAGIMP = mk(typeVoid, sec(2, [1].concat(str("m"), str("e"), [4, 0, 0])));
globalThis.TAGEXP = mk(typeVoid, sec(13, [1, 0, 0]), sec(7, [1].concat(str("e"), [4, 0])));
globalThis.UEXP = mk(typeVoid, sec(3, [1, 0]), sec(7, [1, 2, 0xc3, 0xa9, 0, 0]), sec(10, [1, 2, 0, 0x0b]));
globalThis.__final = function () {
  return __err !== null ? __err : JSON.stringify(log);
};
globalThis.__run = function (src) {
  try {
    (0, eval)(src);
  } catch (e) {
    __err = "error\t" + e.name + "\t" + JSON.stringify(String(e.message));
  }
};
