// Sonda de log1p no ramo k != 0 do s_log1p.c (glibc 2.41, variante FMA): varre as fusões possíveis de `u = 1 + x`
// (soma exata em duplo), de `c`, de `hfsq` e do fim, contra o Math.log1p do bun.
// Uso: bun scripts/log1p-probe-k.js
const buf = new DataView(new ArrayBuffer(8));
const bits = (x) => { buf.setFloat64(0, x); return buf.getBigUint64(0); };
const fromBits = (b) => { buf.setBigUint64(0, b); return buf.getFloat64(0); };
const hex = (x) => bits(x).toString(16).padStart(16, "0");
function decompose(x) {
  buf.setFloat64(0, x);
  const hi = buf.getUint32(0), lo = buf.getUint32(4);
  const sign = hi >>> 31 ? -1n : 1n;
  const e = (hi >>> 20) & 0x7ff;
  let m = (BigInt(hi & 0xfffff) << 32n) | BigInt(lo);
  if (e === 0) return [sign * m, -1074];
  m |= 1n << 52n;
  return [sign * m, e - 1075];
}
function toDouble(m, e) {
  if (m === 0n) return 0;
  const neg = m < 0n; if (neg) m = -m;
  const len = m.toString(2).length;
  if (len > 53) {
    const sh = BigInt(len - 53);
    let q = m >> sh; const rem = m & ((1n << sh) - 1n); const half = 1n << (sh - 1n);
    if (rem > half || (rem === half && (q & 1n))) q += 1n;
    m = q; e += Number(sh);
  }
  const v = Number(m) * Math.pow(2, e);
  return neg ? -v : v;
}
function fma(a, b, c) {
  const [ma, ea] = decompose(a), [mb, eb] = decompose(b), [mc, ec] = decompose(c);
  const mp = ma * mb, ep = ea + eb;
  const e = Math.min(ep, ec);
  return toDouble((mp << BigInt(ep - e)) + (mc << BigInt(ec - e)), e);
}
const LP = [0, 6.666666666666735130e-1, 3.999999999940941908e-1, 2.857142874366239149e-1, 2.222219843214978396e-1, 1.818357216161805012e-1, 1.531383769920937332e-1, 1.479819860511658591e-1];
const LN2_HI = 6.93147180369123816490e-01, LN2_LO = 1.90821492927058770002e-10;
const hiw = (x) => Number(bits(x) >> 32n) | 0;
const setHi = (x, h) => fromBits((BigInt(h >>> 0) << 32n) | (bits(x) & 0xffffffffn));

// v.r: forma do polinômio; v.t: fim do ramo k != 0; v.k0: fim do ramo k == 0
function log1p(x, v) {
  let hx = hiw(x), ax = hx & 0x7fffffff, k = 1, f = 0, hu = 0, c = 0;
  if (hx < 0x3fda827a) {
    if (hx > 0 || hx <= (0xbfd2bec4 | 0)) { k = 0; f = x; hu = 1; }
  }
  if (k !== 0) {
    let u;
    if (hx < 0x43400000) {
      u = 1 + x; hu = hiw(u); k = (hu >> 20) - 1023;
      c = k > 0 ? 1 - (u - x) : x - (u - 1); c /= u;
    } else { u = x; hu = hiw(u); k = (hu >> 20) - 1023; c = 0; }
    hu &= 0xfffff;
    if (hu < 0x6a09e) u = setHi(u, hu | 0x3ff00000);
    else { k += 1; u = setHi(u, hu | 0x3fe00000); hu = (0x100000 - hu) >> 2; }
    f = u - 1;
  }
  const hfsq = 0.5 * f * f;
  const s = f / (2 + f), z = s * s, z2 = z * z, z4 = z2 * z2, z6 = z4 * z2;
  const r2 = fma(z, LP[3], LP[2]), r3 = fma(z, LP[5], LP[4]), r4 = fma(z, LP[7], LP[6]);
  const r = v.r === 0 ? fma(z6, r4, fma(z4, r3, fma(z, LP[1], z2 * r2))) : fma(z6, r4, fma(z4, r3, fma(z2, r2, z * LP[1])));
  if (k === 0) {
    if (v.k0 === 0) return f - fma(-s, hfsq + r, hfsq);
    return f - (hfsq - s * (hfsq + r));
  }
  const kd = k;
  if (v.t === 0) return fma(kd, LN2_HI, -((hfsq - fma(s, hfsq + r, fma(kd, LN2_LO, c))) - f));
  if (v.t === 1) return kd * LN2_HI - ((hfsq - (s * (hfsq + r) + (kd * LN2_LO + c))) - f);
  if (v.t === 2) return fma(kd, LN2_HI, -((hfsq - fma(s, hfsq + r, kd * LN2_LO + c)) - f));
  return fma(kd, LN2_HI, -((hfsq - (s * (hfsq + r) + fma(kd, LN2_LO, c))) - f));
}

console.log("casos conhecidos (esperado bun):");
for (const x of [fromBits(0xbfc899451f000000n), fromBits(0x3fc1eb56b1000000n * 0n + 0x3fc1eb56b1000000n)]) {
  console.log(hex(x), "bun", hex(Math.log1p(x)));
}
let seed = 777;
const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff; };
const variants = [];
for (const r of [0, 1]) for (const t of [0, 1, 2, 3]) for (const k0 of [0, 1]) variants.push({ r, t, k0 });
const xs = [];
for (let i = 0; i < 6000; i++) {
  const sgn = rnd() < 0.5 ? -1 : 1;
  let x = sgn * Math.pow(2, -10 + rnd() * 9.5);
  if (x <= -1) x = -0.5;
  xs.push(x);
  // mantissa curta, como nos casos do golden
  const n = 4 + Math.floor(rnd() * 10);
  xs.push(sgn * Math.round(rnd() * 1e6) / Math.pow(2, n + 12) * 4);
  xs.push(Math.pow(2, rnd() * 40)); // k grande: separa as formas do fim do ramo k != 0
  xs.push(-1 + Math.pow(2, -rnd() * 30)); // perto de -1
}
for (const v of variants) {
  let bad = 0, first = null;
  for (const x of xs) {
    if (x <= -1) continue;
    const got = log1p(x, v), want = Math.log1p(x);
    if (got !== want && !(Number.isNaN(got) && Number.isNaN(want))) { bad++; if (!first) first = hex(x); }
  }
  console.log(JSON.stringify(v), "divergências", bad, "de", xs.length, first ?? "");
}
const g = fromBits(0xbfc899451f000000n);
for (const v of variants) console.log(JSON.stringify(v), hex(log1p(g, v)), "alvo bfcb511c9e247c30");
