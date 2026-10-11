globalThis.log = [];
globalThis.__err = null;
globalThis.L = function (x) { log.push(x); };
globalThis.E = function (e) {
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
globalThis.str = function (s) {
  return [s.length].concat(Array.prototype.map.call(s, function (c) { return c.charCodeAt(0); }));
};
globalThis.sec = function (id, payload) { return [id, payload.length].concat(payload); };
globalThis.mk = function () {
  var out = [0, 97, 115, 109, 1, 0, 0, 0];
  for (var i = 0; i < arguments.length; i++) out = out.concat(arguments[i]);
  return new Uint8Array(out);
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
globalThis.TRAP = mk(typeVoid, sec(3, [1, 0]), sec(7, [1].concat(str("t"), [0, 0])), sec(10, [1, 3, 0, 0x00, 0x0b]));
globalThis.DIV = mk(typeRetI32, sec(3, [1, 0]), sec(7, [1].concat(str("d"), [0, 0])), sec(10, [1, 7, 0, 0x41, 1, 0x41, 0, 0x6d, 0x0b]));
// Módulo de uma função "f" que repassa os parâmetros a uma instrução atômica (prefixo 0xFE, subopcode `sub`, imediatos
// `imm`); a memória é de uma página, compartilhada (com máximo) ou não.
globalThis.AT = function (sub, params, results, imm, shared) {
  var body = [0];
  for (var i = 0; i < params.length; i++) body.push(0x20, i);
  body = body.concat([0xfe, sub], imm, [0x0b]);
  return mk(
    sec(1, [1, 0x60, params.length].concat(params, [results.length], results)),
    sec(3, [1, 0]),
    sec(5, shared ? [1, 3, 1, 1] : [1, 0, 1]),
    sec(7, [1].concat(str("f"), [0, 0])),
    sec(10, [1, body.length].concat(body))
  );
};
// Módulo de uma função "f" (sem locais) com o corpo `body` (sem o `end`), uma memória de uma página exportada como "mem".
globalThis.FN = function (params, results, body) {
  var code = [0].concat(body, [0x0b]);
  return mk(
    sec(1, [1, 0x60, params.length].concat(params, [results.length], results)),
    sec(3, [1, 0]),
    sec(5, [1, 0, 1]),
    sec(7, [2].concat(str("f"), [0, 0], str("mem"), [2, 0])),
    sec(10, [1, code.length].concat(code))
  );
};
globalThis.IMPMEM = mk(sec(2, [1].concat(str("m"), str("mem"), [2, 0, 1])));
globalThis.IMPTAB = mk(sec(2, [1].concat(str("m"), str("tab"), [1, 0x70, 0, 1])));
globalThis.IMPGLB = mk(sec(2, [1].concat(str("m"), str("g"), [3, 0x7f, 0])));
globalThis.CUSTOM = mk(sec(0, str("a").concat([0xaa])), sec(0, str("bc").concat([7])), sec(0, str("a").concat([1, 2, 3])));
globalThis.__final = function () {
  // BigInt não tem forma JSON: vira o texto do literal (`5n`), para o programa poder registrar um i64.
  return __err !== null ? __err : JSON.stringify(log, function (k, v) { return typeof v === "bigint" ? v + "n" : v; });
};
globalThis.__run = function (src) {
  try {
    (0, eval)(src);
  } catch (e) {
    __err = "error\t" + e.name + "\t" + JSON.stringify(String(e.message));
  }
};
