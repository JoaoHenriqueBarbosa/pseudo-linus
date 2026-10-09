// Gera tests/golden/wasm_simd_bun.tsv: programas de SIMD (v128) do WebAssembly, avaliados no bun. Cada programa monta
// à mão um módulo de uma função exportada "f" (parâmetros i64 em pares, resultado multi-value [i64 i64]) com
// `S(...)` de tests/golden/wasm_simd_bun_harness.js (que usa `FN` de wasm_js_bun_harness.js) e registra em `log` o
// array de BigInt em texto, ou `throws <Erro>:<mensagem>` (trap, validação). Os v128 de entrada se constroem com
// i64x2.splat + i64x2.replace_lane a partir dos pares de i64; o resultado v128 sai pela memória (store + dois
// i64.load); resultados escalares (extract_lane, any_true, bitmask) saem estendidos a i64 mais um zero.
// Colunas: fonte, depois o JSON do log. Cada programa roda num processo bun próprio, com timeout. Uso:
//   bun scripts/gen-wasm-simd-golden.js > tests/golden/wasm_simd_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const harness =
  fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_js_bun_harness.js"), "utf8") +
  "\n" +
  fs.readFileSync(path.join(__dirname, "../tests/golden/wasm_simd_bun_harness.js"), "utf8");

const leb = (n) => (n < 0x80 ? [n] : [(n & 0x7f) | 0x80, n >> 7]);
const i32c = (n) => {
  const out = [];
  for (;;) {
    const byte = n & 0x7f;
    n >>= 7;
    if ((n === 0 && !(byte & 0x40)) || (n === -1 && byte & 0x40)) {
      out.push(byte);
      return [0x41, ...out];
    }
    out.push(byte | 0x80);
  }
};
const FD = (sub, ...imm) => [0xfd, ...leb(sub), ...imm];
const hex = (v) => "0x" + BigInt.asUintN(64, v).toString(16) + "n";

// Pools de vetores (pares lo, hi de i64).
const pack = (kind, lanes) => {
  const view = new DataView(new ArrayBuffer(16));
  const size = kind === "f32" ? 4 : 8;
  lanes.forEach((x, i) => {
    if (typeof x === "string") {
      if (kind === "f32") view.setUint32(i * 4, parseInt(x, 16), true);
      else view.setBigUint64(i * 8, BigInt("0x" + x), true);
    } else if (kind === "f32") view.setFloat32(i * 4, x, true);
    else view.setFloat64(i * 8, x, true);
  });
  void size;
  return [view.getBigUint64(0, true), view.getBigUint64(8, true)];
};
const POOLS = {
  i: [
    [0x8081ff7f00017f80n, 0x0102030405060708n],
    [0x8000800080008000n, 0x7fff7fff7fff7fffn],
    [0x8000000080000000n, 0x7fffffff7fffffffn],
    [0x8000000000000000n, 0x7fffffffffffffffn],
    [0xffffffffffffffffn, 0x0000000100000001n],
    [0xfedcba9876543210n, 0x0123456789abcdefn],
    [0n, 0n],
    [0xffffffffffffffffn, 0xffffffffffffffffn],
  ],
  f32: [
    pack("f32", [NaN, -0, Infinity, -Infinity]),
    pack("f32", [1.5, -1.5, 2.5, -2.5]),
    pack("f32", [0.5, -0.5, 3.5, -0.4999]),
    pack("f32", [3e9, -3e9, 4.3e9, 1e-45]),
    pack("f32", [2147483648, -2147483904, 4294967296, 255.5]),
    pack("f32", [3.4028235e38, -3.4028235e38, 1.17549435e-38, 0]),
    pack("f32", ["7fa00000", "ffc00000", -0, 0]),
    pack("f32", [1, 2, 3, 4]),
  ],
  f64: [
    pack("f64", [NaN, -0]),
    pack("f64", [Infinity, -Infinity]),
    pack("f64", [1.5, -1.5]),
    pack("f64", [2.5, 0.5]),
    pack("f64", [4e9, -2147483649]),
    pack("f64", [1.7976931348623157e308, -5e-324]),
    pack("f64", [2147483647.9, -2147483648.9]),
    pack("f64", ["7ff4000000000000", -0.4999999]),
  ],
};
const SCALARS = [0n, -1n, 0x80n, 0x7fffffffn, 0x8000000000000000n, 0xff80000000000000n, 0x7fc00000n, 0x7ff8000000000000n];
const SHIFTS = [0n, 1n, 7n, 8n, 9n, 15n, 16n, 31n, 32n, 33n, 63n, 64n, 65n, -1n];

const programs = [];
const seen = new Set();
const emit = (paramCount, body, args) => {
  const source = `L(T(function(){return S(${paramCount},[${body.join(",")}],[${args.map(hex).join(",")}])}))`;
  if (!seen.has(source)) {
    seen.add(source);
    programs.push(source);
  }
};
const vecLoad = (k) => [0x20, 2 * k, ...FD(0x12), 0x20, 2 * k + 1, ...FD(0x1e, 1)];
const vecStore = (addr, k) => [...i32c(addr), ...vecLoad(k), ...FD(0x0b, 4, 0)];
// compute deixa um v128 na pilha; finish o devolve como dois i64.
const finish = (compute) => [
  ...i32c(64),
  ...compute,
  ...FD(0x0b, 4, 0),
  ...i32c(64),
  0x29, 3, 0,
  ...i32c(64),
  0x29, 3, 8,
];
const finishScalar = (compute, convert) => [...compute, ...convert, 0x42, 0];
const pick = (pool, j, k) => POOLS[pool][(j * 3 + k * [0, 1, 3, 5][j % 4]) % POOLS[pool].length];
const vectorArgs = (pool, j, arity) => {
  const args = [];
  for (let k = 0; k < arity; k++) args.push(...pick(pool, j, k));
  return args;
};

// Instruções de vetor puro.
const ops = [];
const op = (name, sub, kind, pool = "i") => ops.push({ name, sub, kind, pool });
const cmpNames = ["eq", "ne", "lt_s", "lt_u", "gt_s", "gt_u", "le_s", "le_u", "ge_s", "ge_u"];
[["i8x16", 0x23], ["i16x8", 0x2d], ["i32x4", 0x37]].forEach(([shape, base]) =>
  cmpNames.forEach((n, i) => op(`${shape}.${n}`, base + i, "b"))
);
["eq", "ne", "lt", "gt", "le", "ge"].forEach((n, i) => {
  op(`f32x4.${n}`, 0x41 + i, "b", "f32");
  op(`f64x2.${n}`, 0x47 + i, "b", "f64");
});
op("v128.not", 0x4d, "u"); op("v128.and", 0x4e, "b"); op("v128.andnot", 0x4f, "b"); op("v128.or", 0x50, "b");
op("v128.xor", 0x51, "b"); op("v128.bitselect", 0x52, "t"); op("v128.any_true", 0x53, "bool"); op("i8x16.swizzle", 0x0e, "b");
op("f32x4.demote_f64x2_zero", 0x5e, "u", "f64"); op("f64x2.promote_low_f32x4", 0x5f, "u", "f32");
op("i8x16.abs", 0x60, "u"); op("i8x16.neg", 0x61, "u"); op("i8x16.popcnt", 0x62, "u"); op("i8x16.all_true", 0x63, "bool");
op("i8x16.bitmask", 0x64, "bool"); op("i8x16.narrow_i16x8_s", 0x65, "b"); op("i8x16.narrow_i16x8_u", 0x66, "b");
op("f32x4.ceil", 0x67, "u", "f32"); op("f32x4.floor", 0x68, "u", "f32"); op("f32x4.trunc", 0x69, "u", "f32"); op("f32x4.nearest", 0x6a, "u", "f32");
op("i8x16.shl", 0x6b, "sh"); op("i8x16.shr_s", 0x6c, "sh"); op("i8x16.shr_u", 0x6d, "sh");
["add", "add_sat_s", "add_sat_u", "sub", "sub_sat_s", "sub_sat_u"].forEach((n, i) => op(`i8x16.${n}`, 0x6e + i, "b"));
op("f64x2.ceil", 0x74, "u", "f64"); op("f64x2.floor", 0x75, "u", "f64");
op("i8x16.min_s", 0x76, "b"); op("i8x16.min_u", 0x77, "b"); op("i8x16.max_s", 0x78, "b"); op("i8x16.max_u", 0x79, "b");
op("f64x2.trunc", 0x7a, "u", "f64"); op("i8x16.avgr_u", 0x7b, "b");
op("i16x8.extadd_pairwise_i8x16_s", 0x7c, "u"); op("i16x8.extadd_pairwise_i8x16_u", 0x7d, "u");
op("i32x4.extadd_pairwise_i16x8_s", 0x7e, "u"); op("i32x4.extadd_pairwise_i16x8_u", 0x7f, "u");
op("i16x8.abs", 0x80, "u"); op("i16x8.neg", 0x81, "u"); op("i16x8.q15mulr_sat_s", 0x82, "b"); op("i16x8.all_true", 0x83, "bool");
op("i16x8.bitmask", 0x84, "bool"); op("i16x8.narrow_i32x4_s", 0x85, "b"); op("i16x8.narrow_i32x4_u", 0x86, "b");
["extend_low_i8x16_s", "extend_high_i8x16_s", "extend_low_i8x16_u", "extend_high_i8x16_u"].forEach((n, i) => op(`i16x8.${n}`, 0x87 + i, "u"));
op("i16x8.shl", 0x8b, "sh"); op("i16x8.shr_s", 0x8c, "sh"); op("i16x8.shr_u", 0x8d, "sh");
["add", "add_sat_s", "add_sat_u", "sub", "sub_sat_s", "sub_sat_u"].forEach((n, i) => op(`i16x8.${n}`, 0x8e + i, "b"));
op("f64x2.nearest", 0x94, "u", "f64"); op("i16x8.mul", 0x95, "b");
op("i16x8.min_s", 0x96, "b"); op("i16x8.min_u", 0x97, "b"); op("i16x8.max_s", 0x98, "b"); op("i16x8.max_u", 0x99, "b"); op("i16x8.avgr_u", 0x9b, "b");
["extmul_low_i8x16_s", "extmul_high_i8x16_s", "extmul_low_i8x16_u", "extmul_high_i8x16_u"].forEach((n, i) => op(`i16x8.${n}`, 0x9c + i, "b"));
op("i32x4.abs", 0xa0, "u"); op("i32x4.neg", 0xa1, "u"); op("i32x4.all_true", 0xa3, "bool"); op("i32x4.bitmask", 0xa4, "bool");
["extend_low_i16x8_s", "extend_high_i16x8_s", "extend_low_i16x8_u", "extend_high_i16x8_u"].forEach((n, i) => op(`i32x4.${n}`, 0xa7 + i, "u"));
op("i32x4.shl", 0xab, "sh"); op("i32x4.shr_s", 0xac, "sh"); op("i32x4.shr_u", 0xad, "sh");
op("i32x4.add", 0xae, "b"); op("i32x4.sub", 0xb1, "b"); op("i32x4.mul", 0xb5, "b");
op("i32x4.min_s", 0xb6, "b"); op("i32x4.min_u", 0xb7, "b"); op("i32x4.max_s", 0xb8, "b"); op("i32x4.max_u", 0xb9, "b");
op("i32x4.dot_i16x8_s", 0xba, "b");
["extmul_low_i16x8_s", "extmul_high_i16x8_s", "extmul_low_i16x8_u", "extmul_high_i16x8_u"].forEach((n, i) => op(`i32x4.${n}`, 0xbc + i, "b"));
op("i64x2.abs", 0xc0, "u"); op("i64x2.neg", 0xc1, "u"); op("i64x2.all_true", 0xc3, "bool"); op("i64x2.bitmask", 0xc4, "bool");
["extend_low_i32x4_s", "extend_high_i32x4_s", "extend_low_i32x4_u", "extend_high_i32x4_u"].forEach((n, i) => op(`i64x2.${n}`, 0xc7 + i, "u"));
op("i64x2.shl", 0xcb, "sh"); op("i64x2.shr_s", 0xcc, "sh"); op("i64x2.shr_u", 0xcd, "sh");
op("i64x2.add", 0xce, "b"); op("i64x2.sub", 0xd1, "b"); op("i64x2.mul", 0xd5, "b");
["eq", "ne", "lt_s", "gt_s", "le_s", "ge_s"].forEach((n, i) => op(`i64x2.${n}`, 0xd6 + i, "b"));
["extmul_low_i32x4_s", "extmul_high_i32x4_s", "extmul_low_i32x4_u", "extmul_high_i32x4_u"].forEach((n, i) => op(`i64x2.${n}`, 0xdc + i, "b"));
op("f32x4.abs", 0xe0, "u", "f32"); op("f32x4.neg", 0xe1, "u", "f32"); op("f32x4.sqrt", 0xe3, "u", "f32");
["add", "sub", "mul", "div", "min", "max", "pmin", "pmax"].forEach((n, i) => {
  op(`f32x4.${n}`, 0xe4 + i, "b", "f32");
  op(`f64x2.${n}`, 0xf0 + i, "b", "f64");
});
op("f64x2.abs", 0xec, "u", "f64"); op("f64x2.neg", 0xed, "u", "f64"); op("f64x2.sqrt", 0xef, "u", "f64");
op("i32x4.trunc_sat_f32x4_s", 0xf8, "u", "f32"); op("i32x4.trunc_sat_f32x4_u", 0xf9, "u", "f32");
op("f32x4.convert_i32x4_s", 0xfa, "u"); op("f32x4.convert_i32x4_u", 0xfb, "u");
op("i32x4.trunc_sat_f64x2_s_zero", 0xfc, "u", "f64"); op("i32x4.trunc_sat_f64x2_u_zero", 0xfd, "u", "f64");
op("f64x2.convert_low_i32x4_s", 0xfe, "u"); op("f64x2.convert_low_i32x4_u", 0xff, "u");
// relaxed-simd
op("i8x16.relaxed_swizzle", 0x100, "b");
op("i32x4.relaxed_trunc_f32x4_s", 0x101, "u", "f32"); op("i32x4.relaxed_trunc_f32x4_u", 0x102, "u", "f32");
op("i32x4.relaxed_trunc_f64x2_s_zero", 0x103, "u", "f64"); op("i32x4.relaxed_trunc_f64x2_u_zero", 0x104, "u", "f64");
op("f32x4.relaxed_madd", 0x105, "t", "f32"); op("f32x4.relaxed_nmadd", 0x106, "t", "f32");
op("f64x2.relaxed_madd", 0x107, "t", "f64"); op("f64x2.relaxed_nmadd", 0x108, "t", "f64");
["i8x16", "i16x8", "i32x4", "i64x2"].forEach((s, i) => op(`${s}.relaxed_laneselect`, 0x109 + i, "t"));
op("f32x4.relaxed_min", 0x10d, "b", "f32"); op("f32x4.relaxed_max", 0x10e, "b", "f32");
op("f64x2.relaxed_min", 0x10f, "b", "f64"); op("f64x2.relaxed_max", 0x110, "b", "f64");
op("i16x8.relaxed_q15mulr_s", 0x111, "b"); op("i16x8.relaxed_dot_i8x16_i7x16_s", 0x112, "b"); op("i32x4.relaxed_dot_i8x16_i7x16_add_s", 0x113, "t");

for (const o of ops) {
  const cases = o.pool === "i" ? 3 : 4;
  for (let j = 0; j < cases; j++) {
    const jj = j + (o.sub % 3);
    if (o.kind === "u" || o.kind === "b" || o.kind === "t") {
      const arity = { u: 1, b: 2, t: 3 }[o.kind];
      const body = [];
      for (let k = 0; k < arity; k++) body.push(...vecLoad(k));
      emit(arity * 2, finish([...body.slice(0, 0), ...body, ...FD(o.sub)]), vectorArgs(o.pool, jj, arity));
    } else if (o.kind === "bool") {
      emit(2, finishScalar([...vecLoad(0), ...FD(o.sub)], [0xac]), vectorArgs(o.pool, jj, 1));
    } else {
      const shifts = [SHIFTS[(jj * 5) % SHIFTS.length], SHIFTS[(jj * 5 + 3) % SHIFTS.length]];
      // o contador de shift é o terceiro parâmetro (i64), cortado a i32.
      emit(3, finish([...vecLoad(0), 0x20, 2, 0xa7, ...FD(o.sub)]), [...vectorArgs(o.pool, jj, 1), shifts[j % 2]]);
    }
  }
}

// v128.const e shuffle.
for (let j = 0; j < 4; j++) {
  const [lo, hi] = POOLS.i[j * 2];
  const view = new DataView(new ArrayBuffer(16));
  view.setBigUint64(0, lo, true);
  view.setBigUint64(8, hi, true);
  emit(0, finish([...FD(0x0c, ...new Uint8Array(view.buffer))]), []);
}
const SHUFFLES = [
  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
  [16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
  [31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16],
  [0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23],
  [15, 15, 15, 15, 0, 0, 0, 0, 31, 31, 31, 31, 16, 16, 16, 16],
  [3, 7, 11, 15, 19, 23, 27, 31, 2, 6, 10, 14, 18, 22, 26, 30],
];
SHUFFLES.forEach((lanes, j) => emit(4, finish([...vecLoad(0), ...vecLoad(1), ...FD(0x0d, ...lanes)]), vectorArgs("i", j, 2)));

// splat (parâmetro escalar i64 convertido ao tipo da lane).
[
  ["i8x16.splat", 0x0f, [0xa7]], ["i16x8.splat", 0x10, [0xa7]], ["i32x4.splat", 0x11, [0xa7]], ["i64x2.splat", 0x12, []],
  ["f32x4.splat", 0x13, [0xa7, 0xbe]], ["f64x2.splat", 0x14, [0xbf]],
].forEach(([, sub, conv]) =>
  [0, 1, 2, 3, 4, 5, 6, 7].forEach((j) => {
    if (j % 2 && sub < 0x13) return;
    emit(1, finish([0x20, 0, ...conv, ...FD(sub)]), [SCALARS[j]]);
  })
);

// extract_lane (resultado escalar) e replace_lane (vetor).
const LANE_SHAPES = [
  ["i8x16.extract_lane_s", 0x15, 0x17, 16, "i", [0xac], [0x20, 2, 0xa7]],
  ["i8x16.extract_lane_u", 0x16, 0x17, 16, "i", [0xad], [0x20, 2, 0xa7]],
  ["i16x8.extract_lane_s", 0x18, 0x1a, 8, "i", [0xac], [0x20, 2, 0xa7]],
  ["i16x8.extract_lane_u", 0x19, 0x1a, 8, "i", [0xad], [0x20, 2, 0xa7]],
  ["i32x4.extract_lane", 0x1b, 0x1c, 4, "i", [0xac], [0x20, 2, 0xa7]],
  ["i64x2.extract_lane", 0x1d, 0x1e, 2, "i", [], [0x20, 2]],
  ["f32x4.extract_lane", 0x1f, 0x20, 4, "f32", [0xbc, 0xad], [0x20, 2, 0xa7, 0xbe]],
  ["f64x2.extract_lane", 0x21, 0x22, 2, "f64", [0xbd], [0x20, 2, 0xbf]],
];
for (const [, extract, replace, lanes, pool, conv, scalar] of LANE_SHAPES) {
  [0, lanes - 1, 1 % lanes, (lanes >> 1) % lanes].forEach((lane, j) => {
    emit(2, finishScalar([...vecLoad(0), ...FD(extract, lane)], conv), vectorArgs(pool, j + lane, 1));
    if (extract === 0x16 || extract === 0x19) return; // replace_lane tem uma instrução só por forma
    emit(3, finish([...vecLoad(0), ...scalar, ...FD(replace, lane)]), [...vectorArgs(pool, j + lane, 1), SCALARS[(j * 3 + lane) % SCALARS.length]]);
  });
}

// Memória: loads (com extensão, splat, zero), lane e store. Pré-carga: vetor A em 0 e vetor B em 16.
const prefill = [...vecStore(0, 0), ...vecStore(16, 1)];
const LOADS = [
  [0x00, 4], [0x01, 3], [0x02, 3], [0x03, 3], [0x04, 3], [0x05, 3], [0x06, 3],
  [0x07, 0], [0x08, 1], [0x09, 2], [0x0a, 3], [0x5c, 2], [0x5d, 3],
];
const ADDRS = [[0, 0], [1, 0], [3, 1], [8, 5]];
for (const [sub, align] of LOADS) {
  ADDRS.forEach(([addr, offset], j) =>
    emit(4, [...prefill, ...finish([...i32c(addr), ...FD(sub, align, offset)])], vectorArgs("i", j + sub, 2))
  );
}
// Fora dos limites (uma página): traps.
[[0x00, 4, 65521], [0x00, 4, 65535], [0x07, 0, 65536], [0x0a, 3, 65529], [0x5d, 3, 65529], [0x01, 3, 65529]].forEach(([sub, align, addr], j) =>
  emit(4, [...prefill, ...finish([...i32c(addr), ...FD(sub, align, 0)])], vectorArgs("i", j, 2))
);
// Limite exato (válido): v128.load em 65520.
emit(4, [...prefill, ...finish([...i32c(65520), ...FD(0x00, 4, 0)])], vectorArgs("i", 1, 2));
emit(4, [...prefill, ...finish([...i32c(65500), ...FD(0x00, 4, 20)])], vectorArgs("i", 2, 2));
// load_lane (v128 B, endereço e lane) e store_lane; v128.store.
const LANE_MEM = [[0x54, 0, 16, 0x58], [0x55, 1, 8, 0x59], [0x56, 2, 4, 0x5a], [0x57, 3, 2, 0x5b]];
for (const [loadSub, align, lanes, storeSub] of LANE_MEM) {
  [0, lanes - 1, 1 % lanes].forEach((lane, j) => {
    emit(4, [...prefill, ...finish([...i32c(j * 3), ...vecLoad(1), ...FD(loadSub, align, 0, lane)])], vectorArgs("i", j + lane, 2));
    emit(
      4,
      [...prefill, ...i32c(32 + j), ...vecLoad(1), ...FD(storeSub, align, 0, lane), ...finish([...i32c(32), ...FD(0x00, 4, 0)])],
      vectorArgs("i", j + lane + 1, 2)
    );
  });
}
[[37, 2], [32, 0], [41, 7]].forEach(([addr, offset], j) =>
  emit(4, [...prefill, ...i32c(addr), ...vecLoad(1), ...FD(0x0b, 4, offset), ...finish([...i32c(32), ...FD(0x00, 4, 0)])], vectorArgs("i", j + 2, 2))
);

// Validação: lane fora do intervalo e tipos errados são erros de CompileError.
emit(2, [...finishScalar([...vecLoad(0), ...FD(0x15, 16)], [0xac])], vectorArgs("i", 0, 1));
emit(2, [...finishScalar([...vecLoad(0), ...FD(0x21, 2)], [0xbd])], vectorArgs("f64", 0, 1));

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-simd-golden-"));
const lines = [];
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script = harness + `\n__run(${JSON.stringify(source)});\n{ const out = __final(); process.stdout.write(out); }\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
