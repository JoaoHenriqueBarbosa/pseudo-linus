// Prelúdio do golden de coerção (gerado por scripts/gen-coercion-golden.js).
class Pair { constructor(result, after) { this.result = result; this.after = after; } }
const F = [
  () => (undefined),
  () => (null),
  () => (true),
  () => (false),
  () => (0),
  () => (-0),
  () => (1),
  () => (-1),
  () => (NaN),
  () => (Infinity),
  () => (-Infinity),
  () => (2 ** 31),
  () => (2 ** 32),
  () => (2 ** 53),
  () => (1e21),
  () => (1e-7),
  () => (0.1),
  () => (''),
  () => (' '),
  () => ('0'),
  () => ('1'),
  () => ('1e3'),
  () => ('0x10'),
  () => ('0b11'),
  () => ('-0'),
  () => ('abc'),
  () => ('  42  '),
  () => ('\n'),
  () => ([]),
  () => ([0]),
  () => ([1, 2]),
  () => ({}),
  () => ({ valueOf() { return 7 } }),
  () => ({ toString() { return '8' } }),
  () => ({ valueOf: null, toString() { return 'x' } }),
  () => (Symbol()),
  () => (1n),
  () => (-1n),
  () => (0n),
  () => (2n ** 64n),
  () => (function () {}),
  () => (new Date(0)),
  () => (new String('s')),
  () => (new Number(3)),
  () => (new Boolean(false)),
  () => ({ [Symbol.toPrimitive](hint) { return hint === 'default' ? 10 : hint === 'number' ? 20 : 30 } }),
  () => ({ [Symbol.toPrimitive](hint) { return hint } }),
  () => ({ valueOf() { throw new RangeError('boom') } }),
  () => (new Proxy({}, {})),
  () => (Object.create(null)),
  () => (new Proxy(function () {}, {})),
  () => (-(2 ** 31)),
  () => (0.5),
  () => (-1.5),
  () => (Number.MAX_VALUE),
  () => (Number.MIN_VALUE),
  () => ('9007199254740993'),
  () => ([[]]),
  () => ([null]),
  () => ({ [Symbol.toPrimitive]() { return {} } }),
];
function mk(index) { return F[index](); }
function ser(v) {
  if (v instanceof Pair) return ser(v.result) + " | " + ser(v.after);
  switch (typeof v) {
    case "undefined": return "undefined";
    case "boolean": return "boolean " + v;
    case "number": return "number " + (Object.is(v, -0) ? "-0" : String(v));
    case "string": return "string " + JSON.stringify(v);
    case "bigint": return "bigint " + String(v);
    case "symbol": return "symbol " + Symbol.prototype.toString.call(v);
    case "function": return "function";
    default:
      if (v === null) return "null";
      if (Array.isArray(v)) return "array " + v.length;
      return "object " + Object.prototype.toString.call(v);
  }
}
