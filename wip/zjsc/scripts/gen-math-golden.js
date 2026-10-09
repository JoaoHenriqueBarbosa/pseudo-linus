// Gera tests/golden/math_bun.tsv: bits dos doubles em hexadecimal, rodado no bun (o oráculo, glibc).
// Colunas: função, argumentos (hexadecimal separados por vírgula), bits do resultado. NaN sai
// canônico (7ff8000000000000), porque o JavaScript não expõe o padrão de bits do NaN.
// Uso: bun scripts/gen-math-golden.js > tests/golden/math_bun.tsv
const buf = new DataView(new ArrayBuffer(8));
const CANONICAL_NAN = "7ff8000000000000";
function hex(x) {
  if (Number.isNaN(x)) return CANONICAL_NAN;
  buf.setFloat64(0, x);
  return buf.getUint32(0).toString(16).padStart(8, "0") + buf.getUint32(4).toString(16).padStart(8, "0");
}
let seed = 0x2545f491;
function rnd() { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return seed >>> 0; }
function rndDouble() { buf.setUint32(0, rnd()); buf.setUint32(4, rnd()); return buf.getFloat64(0); }

// 200 argumentos: especiais, subnormais, potências, valores de redução trigonométrica e aleatórios.
const vals = [0, -0, 1, -1, 0.5, -0.5, 2, -2, 3, 10, 0.1, -0.1, 1 / 3, Math.PI, -Math.PI, Math.PI / 2, Math.PI / 4,
  Math.E, 5e-324, -5e-324, 2.2250738585072014e-308, 2.225073858507201e-308, 1e-310, 1.7976931348623157e308,
  -1.7976931348623157e308, Infinity, -Infinity, NaN, 1e-7, 1e21, 1e22, 710, -745, 709.782712893384, -708.3964185322641,
  1e300, 1e-300, 0.9999999999999999, 1.0000000000000002, 2 ** 31, 2 ** 32, 2 ** 53, -(2 ** 53), 4294967295.5,
  65504, 65520, 1e5, 1e6, 1e9, 1e15, 1e16, 100, 1000, 0.7853981633974483, 1.5707963267948966, 3.141592653589793,
  6.283185307179586, 1e-5, 0.3, 0.7, 12345.6789, -12345.6789];
while (vals.length < 160) vals.push(rndDouble());
while (vals.length < 200) vals.push((rnd() % 2000001 - 1000000) / Math.pow(10, rnd() % 8));

const unary = ["abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "cbrt", "ceil", "clz32", "cos", "cosh", "exp",
  "expm1", "floor", "fround", "log", "log10", "log1p", "log2", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh",
  "trunc", "f16round"];
const out = [];
function row(name, args, result) { out.push([name, args.map(hex).join(","), hex(result)].join("\t")); }

for (const name of unary) {
  if (typeof Math[name] !== "function") continue;
  for (const x of vals) row(name, [x], Math[name](x));
}
// Binárias: pares embaralhados dos 200 valores mais os cantos especiais.
const specials = [0, -0, 1, -1, 0.5, -0.5, 2, -2, Infinity, -Infinity, NaN, 5e-324, 1e308, 3, -3, 1e-310];
const pairs = [];
for (const a of specials) for (const b of specials) pairs.push([a, b]);
for (let i = 0; i < 200; i++) pairs.push([vals[rnd() % vals.length], vals[rnd() % vals.length]]);
for (const name of ["atan2", "pow", "imul", "max", "min", "hypot"]) {
  for (const [a, b] of pairs) row(name, [a, b], Math[name](a, b));
}
// pow com expoente inteiro pequeno e meio (ramos de operationMathPow).
for (const a of vals.slice(0, 80)) for (const b of [0.5, -0.5, 2, 3, 7, 1024, -1, -2, 1e10]) row("pow", [a, b], Math.pow(a, b));
// hypot com três e quatro argumentos.
for (let i = 0; i < 80; i++) {
  const t = [vals[rnd() % vals.length], vals[rnd() % vals.length], vals[rnd() % vals.length]];
  row("hypot", t, Math.hypot(...t));
  const q = [...t, vals[rnd() % vals.length]];
  row("hypot", q, Math.hypot(...q));
}
if (typeof Math.sumPrecise === "function") {
  for (let i = 0; i < 80; i++) {
    const t = [vals[rnd() % vals.length], vals[rnd() % vals.length], vals[rnd() % vals.length]];
    row("sumPrecise", t, Math.sumPrecise(t));
  }
}

// Casos adicionais por função (sempre depois dos antigos, para não deslocar a sequência aleatória).
const next = (x) => { buf.setFloat64(0, x); const b = buf.getBigUint64(0); buf.setBigUint64(0, x < 0 ? b - 1n : b + 1n); return buf.getFloat64(0); };
const prev = (x) => -next(-x);
function pick(a) { return a[rnd() % a.length]; }
function unit() { return rnd() / 4294967296; }
// Potências de dois de 2^-1074 a 2^1023 (subnormais e normais), e vizinhos.
const pow2 = [];
for (let k = -1074; k <= 1023; k += 7) pow2.push(2 ** k);
// Múltiplos de pi/2 (e vizinhos) que passam pelo redutor de argumento: pequenos, médios (|x| < 2^20 pi/2),
// grandes (Payne-Hanek) e perto dos limiares do __ieee754_rem_pio2 (pi/4, 3pi/4, 5pi/4, 9pi/4, 2^19 pi/2).
const piHalf = [];
for (let k = -40; k <= 40; k++) piHalf.push(k * Math.PI / 2);
for (const k of [100, 255, 1000, 65535, 1 << 19, (1 << 19) + 1, 1 << 20, 1e6, 1e8, 2 ** 31, 2 ** 40, 1e15, 2 ** 52]) piHalf.push(k * Math.PI / 2, -k * Math.PI / 2);
for (const x of [Math.PI / 4, 3 * Math.PI / 4, 5 * Math.PI / 4, 7 * Math.PI / 4, 9 * Math.PI / 4, 2 ** 28, 2 ** 27, 105414357, 1647099.3291652855, 6.283185307179586e8]) piHalf.push(x, -x);
const nearPi = piHalf.flatMap((x) => [x, next(x), prev(x)]);
const trigRange = [];
for (let i = 0; i < 80; i++) trigRange.push((unit() * 2 - 1) * 2 ** (rnd() % 60 - 5));
for (let i = 0; i < 40; i++) trigRange.push((unit() * 2 - 1) * 1e308);
const extra = {
  sin: [...pow2, ...nearPi, ...trigRange], cos: [...pow2, ...nearPi, ...trigRange], tan: [...pow2, ...nearPi, ...trigRange],
  asin: [], acos: [], atanh: [], acosh: [], atan: [], asinh: [], sinh: [], cosh: [], tanh: [], exp: [], expm1: [],
  log: [], log2: [], log10: [], log1p: [], cbrt: [], fround: [], clz32: [],
};
const unitDomain = () => { const s = rnd() % 4; return (unit() * 2 - 1) * (s === 0 ? 1 : s === 1 ? 2 ** -(rnd() % 60) : s === 2 ? 0.5 : 1 - 2 ** -(rnd() % 53)); };
for (let i = 0; i < 300; i++) {
  extra.asin.push(unitDomain()); extra.acos.push(unitDomain()); extra.atanh.push(unitDomain());
  extra.acosh.push(1 + unit() * 2 ** (rnd() % 40 - 20)); extra.atan.push((unit() * 2 - 1) * 2 ** (rnd() % 120 - 60));
  extra.asinh.push((unit() * 2 - 1) * 2 ** (rnd() % 120 - 60)); extra.sinh.push((unit() * 2 - 1) * 2 ** (rnd() % 12 - 6) * 3);
  extra.cosh.push((unit() * 2 - 1) * 2 ** (rnd() % 12 - 6) * 3); extra.tanh.push((unit() * 2 - 1) * 2 ** (rnd() % 10 - 5) * 4);
  extra.exp.push((unit() * 2 - 1) * 2 ** (rnd() % 12 - 6) * 12); extra.expm1.push((unit() * 2 - 1) * 2 ** (rnd() % 14 - 8) * 6);
  extra.log.push(2 ** (rnd() % 2098 - 1074) * (1 + unit())); extra.log2.push(2 ** (rnd() % 2098 - 1074) * (1 + unit()));
  extra.log10.push(10 ** (rnd() % 600 - 300) * (1 + unit() * 9)); extra.log1p.push((unit() * 2 - 1) * 2 ** (rnd() % 60 - 55));
  extra.cbrt.push((unit() * 2 - 1) * 2 ** (rnd() % 2098 - 1074)); extra.fround.push((unit() * 2 - 1) * 2 ** (rnd() % 300 - 150));
}
// Pontos de corte conhecidos das funções do glibc.
extra.exp.push(709.782712893384, 709.7827128933841, -745.1332191019411, -745.1332191019412, -708.3964185322641, 0.5 * Math.LN2, 1.5 * Math.LN2, 2 ** -28, 2 ** -54, 1e-300);
extra.expm1.push(709.782712893384, 709.7827128933841, -38 * Math.LN2, 56 * Math.LN2, 0.5 * Math.LN2, 1.5 * Math.LN2, 2 ** -54, 2 ** -55);
extra.sinh.push(22, 22.000000000000004, 709.782712893384, 710.4758600739439, 710.4758600739440, 2 ** -28, 2 ** -29);
extra.cosh.push(0.34657359027997264, 22, 709.782712893384, 710.4758600739439, 710.4758600739440);
extra.tanh.push(22, 2 ** -55, 2 ** -56, 1e-7, 40, 19.061547465398496);
extra.acosh.push(1, 2, 2 ** 28, 2 ** 29, 1.0000000000000002, 1e300);
extra.atanh.push(0.5, 2 ** -28, 2 ** -29, 1 - 2 ** -53, -(1 - 2 ** -53));
extra.log1p.push(2 ** -54, 2 ** -53, 2 ** -29, 2 ** -28, Math.SQRT2 - 1, (Math.SQRT2 - 1) / 2, -0.2928932188134525, -(2 ** -53), -(1 - 2 ** -53), 2 ** 53, 2 ** 54);
extra.log.push(...pow2, 0.7071067811865476, 1.4142135623730951, 0.9999999999999999, 1.0000000000000002);
extra.log2.push(...pow2, 3, 5, 7, 1.0000000000000002, 0.9999999999999999);
extra.log10.push(...pow2, 10, 100, 1e22, 1e-22, 1e23, 1000, 1e300);
extra.cbrt.push(...pow2, 8, 27, -8, 64, 1e-300, 1e300);
// O bun devolve cbrt corretamente arredondado: cubos perfeitos, negativos, subnormais, especiais e 2 e 10 (o
// `__cbrt` antigo errava em 1 ulp em 27, 2 e 10).
extra.cbrt.push(1, -1, 125, -27, 0.001, 2, 3, 10, 1000, 1e6, 0, -0, NaN, Infinity, -Infinity, 5e-324, -5e-324, 1e-310, 2.2250738585072014e-308, 1.7976931348623157e308);
for (let k = 1; k <= 30; k++) extra.cbrt.push(k ** 3, -(k ** 3), k ** 3 + 1, k ** 3 - 1);
for (let i = 0; i < 100; i++) extra.cbrt.push(rnd() % 1000000 / 1000);
extra.fround.push(3.4028234663852886e38, 3.4028235677973366e38, 3.402823567797337e38, 1.401298464324817e-45, 7.006492321624085e-46, 7.006492321624087e-46, 1.1754943508222875e-38, 16777217, 16777219, 0.1, 0.3);
for (const x of [0, 1, 2, 3, 255, 256, 65535, 65536, 2 ** 31 - 1, 2 ** 31, 2 ** 32 - 1, 2 ** 32, 2 ** 32 + 1, 2 ** 53, -1, -(2 ** 31), -(2 ** 32) - 1, 0.5, 1.9, -1.9, NaN, Infinity, -Infinity, 1e300, "7", null, undefined]) extra.clz32.push(x);
for (let k = 0; k < 40; k++) extra.clz32.push(2 ** k, 2 ** k - 1, 2 ** k + 1, -(2 ** k));
for (let i = 0; i < 100; i++) extra.clz32.push(rnd(), rnd() * 4294967296 + rnd(), (rnd() | 0) / 7);
for (const name of Object.keys(extra)) {
  for (const x of extra[name]) {
    if (typeof x !== "number") { if (name === "clz32") row(name, [Number(x)], Math[name](x)); continue; }
    row(name, [x], Math[name](x));
  }
}
// atan2: eixos, quadrantes, razões extremas, subnormais e potências de dois.
for (const a of [0, -0, 1, -1, 5e-324, 1e-310, 1e308, Infinity, -Infinity, 2 ** -60, 2 ** 60, 3, 1.7976931348623157e308])
  for (const b of [0, -0, 1, -1, 5e-324, -5e-324, 1e-310, 1e308, -1e308, Infinity, -Infinity, 2 ** -60, 2 ** 60, 2 ** 100, 2 ** -100, 3, NaN]) row("atan2", [a, b], Math.atan2(a, b));
for (let i = 0; i < 300; i++) { const a = (unit() * 2 - 1) * 2 ** (rnd() % 200 - 100), b = (unit() * 2 - 1) * 2 ** (rnd() % 200 - 100); row("atan2", [a, b], Math.atan2(a, b)); }
// pow: base e expoente inteiros e fracionários, resultados exatos, overflow e underflow, potências de dois.
for (let i = 0; i < 400; i++) {
  const s = rnd() % 4;
  const a = s === 0 ? pick(pow2) : s === 1 ? unit() * 20 : s === 2 ? -(unit() * 20) : (unit() * 2 - 1) * 2 ** (rnd() % 20 - 10);
  const b = rnd() % 2 ? Math.round((unit() * 2 - 1) * 2000) : (unit() * 2 - 1) * 2 ** (rnd() % 12 - 4);
  row("pow", [a, b], Math.pow(a, b));
}
for (const a of [0.9999999999999999, 1.0000000000000002, 1.0000000001, 2, 10, 0.5, -2, 1e-300, 1e300, 1.1, 0.1]) for (const b of [2 ** 53, -(2 ** 53), 1e15, 2 ** 31, 1 / 3, 0.1, 100, -100, 308, 309, -323, -324, 1023, 1024, 1074, -1074, -1075]) row("pow", [a, b], Math.pow(a, b));
// hypot: dois argumentos em escalas muito diferentes, subnormais, e três e quatro ou mais argumentos.
for (let i = 0; i < 300; i++) {
  const a = (unit() * 2 - 1) * 2 ** (rnd() % 2098 - 1074), b = (unit() * 2 - 1) * 2 ** (rnd() % 2098 - 1074);
  row("hypot", [a, b], Math.hypot(a, b));
  const c = (unit() * 2 - 1) * 2 ** (rnd() % 400 - 200), d = (unit() * 2 - 1) * 2 ** (rnd() % 400 - 200), e = (unit() * 2 - 1) * 2 ** (rnd() % 400 - 200);
  row("hypot", [c, d, e], Math.hypot(c, d, e));
  if (i < 150) { const f = [c, d, e, a, b].slice(0, 4 + (i % 2)); row("hypot", f, Math.hypot(...f)); }
}
for (const t of [[], [3], [-0], [NaN], [Infinity, NaN, 1], [NaN, NaN, NaN], [1e308, 1e308, 1e308], [5e-324, 5e-324, 5e-324], [3, 4, 12], [1, 1, 1, 1, 1, 1, 1, 1, 1]]) row("hypot", t, Math.hypot(...t));
// imul: inteiros de 32 bits, sinal, estouro e fracionários.
for (let i = 0; i < 300; i++) {
  const a = [rnd() | 0, rnd(), (rnd() | 0) / 3, 2 ** (rnd() % 40), -(2 ** (rnd() % 33))][rnd() % 5], b = [rnd() | 0, rnd(), (rnd() | 0) / 3, 2 ** (rnd() % 40), 0xffffffff][rnd() % 5];
  row("imul", [a, b], Math.imul(a, b));
}
// sumPrecise: cancelamento, subnormais, overflow intermediário e listas de tamanhos variados.
if (typeof Math.sumPrecise === "function") {
  for (let i = 0; i < 120; i++) {
    const n = rnd() % 12;
    const t = [];
    for (let j = 0; j < n; j++) t.push([vals[rnd() % vals.length], (unit() * 2 - 1) * 2 ** (rnd() % 2098 - 1074), 1e308, -1e308, 0.1, -0.1][rnd() % 6]);
    if (i % 3 === 0) t.push(...t.map((x) => -x));
    row("sumPrecise", t, Math.sumPrecise(t));
  }
  for (const t of [[], [-0], [-0, -0], [0, -0], [1e308, 1e308], [1e308, 1e308, -1e308], [Infinity, -Infinity], [Infinity, 1], [NaN, 1], [0.1, 0.2, 0.3], [1, 1e100, 1, -1e100], [5e-324, 5e-324], [2 ** 53, 1, 1]]) row("sumPrecise", t, Math.sumPrecise(t));
}
console.log(out.join("\n"));
