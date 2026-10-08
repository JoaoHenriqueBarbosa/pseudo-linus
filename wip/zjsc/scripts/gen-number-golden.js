// Gera tests/golden/number_to_string.tsv: bits do double em hexadecimal, String(x),
// x.toFixed(k), x.toPrecision(p) e x.toExponential(k), rodado no bun 1.4.2 (o oráculo).
const buf = new DataView(new ArrayBuffer(8));
let seed = 0x2545f491;
function rnd() { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return seed >>> 0; }
const vals = [0, -0, 1, -1, 0.1, 0.5, 1/3, 2/3, 1e21, 1e-7, 123456789012345680000, 5e-324,
  1.7976931348623157e308, Math.PI, Math.E, 100, 1e100, 0.000001, 123.456, -987.654e-12, NaN, Infinity, -Infinity];
for (let i = 0; i < 3000; i++) { buf.setUint32(0, rnd()); buf.setUint32(4, rnd()); vals.push(buf.getFloat64(0)); }
for (let i = 0; i < 1000; i++) vals.push((rnd() % 2000000 - 1000000) / Math.pow(10, rnd() % 12));
const out = [];
for (const x of vals) {
  buf.setFloat64(0, x);
  const hex = buf.getUint32(0).toString(16).padStart(8, "0") + buf.getUint32(4).toString(16).padStart(8, "0");
  const k = rnd() % 21, p = 1 + rnd() % 21;
  const f = (g) => { try { return g(); } catch (e) { return "!" + e.constructor.name; } };
  out.push([hex, String(x), f(() => x.toFixed(k)), k, f(() => x.toPrecision(p)), p, f(() => x.toExponential(k))].join("\t"));
}
console.log(out.join("\n"));
