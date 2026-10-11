// Gera tests/golden/wasm_numeric_bun.tsv: instruções numéricas de WebAssembly (i32, i64, f32, f64, conversões, SIMD v128,
// memória) avaliadas no bun, com entradas de borda (0, 1, -1, máximo, mínimo, NaN, Inf, subnormais, -0) e BigInt para i64.
// Os módulos saem de wat via `wasm-tools parse` e entram no fonte de cada programa em hexadecimal (`HX("...")`).
// Colunas: fonte, depois o JSON do log, ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona. Cada
// programa roda num processo bun próprio. Uso:
//   bun scripts/gen-wasm-numeric-golden.js > tests/golden/wasm_numeric_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const golden = path.join(__dirname, "../tests/golden");
const harness = ["wasm_js_bun_harness.js", "wasm_exceptions_bun_extra.js", "wasm_gc_bun_extra.js", "wasm_numeric_bun_extra.js"]
  .map((name) => fs.readFileSync(path.join(golden, name), "utf8"))
  .join("\n");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-numeric-golden-"));

let moduleCount = 0;
const hexOf = (wat) => {
  const file = path.join(tmp, `m${moduleCount++}.wat`);
  fs.writeFileSync(file, wat);
  const parsed = spawnSync("wasm-tools", ["parse", file, "-o", file + ".wasm"], { encoding: "utf8" });
  if (parsed.status !== 0) throw new Error("wat inválido:\n" + parsed.stderr + "\n" + wat);
  return fs.readFileSync(file + ".wasm").toString("hex");
};

const programs = [];
const seen = new Set();
const add = (source) => {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  if (!seen.has(source)) {
    seen.add(source);
    programs.push(source);
  }
};
const prefixOf = (wat) => `var x = RUN(HX("${hexOf(wat)}")); `;
// Divide uma lista de chamadas em programas de `chunk` chamadas cada.
const emit = (prefix, items, chunk) => {
  for (let i = 0; i < items.length; i += chunk) add(prefix + items.slice(i, i + chunk).join("; "));
};
const callItems = (name, argLists) => argLists.map((args) => `L(Z(() => x.${name}(${args.join(", ")})))`);
const calls = (prefix, name, argLists, chunk) => emit(prefix, callItems(name, argLists), chunk);
const one = (values) => values.map((v) => [v]);

// ---------- valores de borda ----------
const I32 = [0, 1, -1, 2, 7, 31, 255, 128, 32768, 65535, 65536, 2147483647, -2147483648, 4294967295, 1431655765, -2147483647];
const I32_PAIRS = [[0, 0], [1, 1], [1, -1], [-1, -1], [2147483647, 1], [-2147483648, -1], [-2147483648, 1], [4294967295, 2], [7, 0], [0, 7],
  [5, 32], [5, 33], [-8, 1], [-8, 31], [1431655765, 3], [-1, 32], [-1, 100], [1, -31], [3, 4294967297], [1.5, 2.5], ["'3'", "'4'"], ["null", "undefined"]];
const I64 = ["0n", "1n", "-1n", "2n", "7n", "63n", "255n", "128n", "32768n", "65536n", "2147483648n", "4294967296n", "4294967295n", "2n ** 63n - 1n",
  "-(2n ** 63n)", "2n ** 64n - 1n", "0x5555555555555555n", "-(2n ** 31n)", "2n ** 32n + 255n"];
const I64_PAIRS = [["0n", "0n"], ["1n", "1n"], ["1n", "-1n"], ["-1n", "-1n"], ["2n ** 63n - 1n", "1n"], ["-(2n ** 63n)", "-1n"], ["-(2n ** 63n)", "1n"],
  ["2n ** 64n - 1n", "2n"], ["7n", "0n"], ["0n", "7n"], ["5n", "64n"], ["5n", "65n"], ["-8n", "1n"], ["-8n", "63n"], ["0x5555555555555555n", "3n"],
  ["1n", "2n ** 64n + 1n"], ["-1n", "100n"], ["1", "2"], ["'3'", "'4'"], ["1n", "2"]];
const F_SPECIAL = ["0", "-0", "1", "-1", "0.5", "-0.5", "1.5", "-1.5", "2.5", "-2.5", "3.5", "NaN", "Infinity", "-Infinity", "4", "2"];
const F64_UN = [...F_SPECIAL, "5e-324", "-5e-324", "1.7976931348623157e308", "0.49999999999999994", "4503599627370497.5", "1e300", "0.1"];
const F32_UN = [...F_SPECIAL, "1e-45", "-1e-45", "3.4028235e38", "1.17549435e-38", "16777217", "8388608.5", "0.1"];
const F64_PAIRS = [["0", "0"], ["-0", "0"], ["0", "-0"], ["-0", "-0"], ["1", "-1"], ["1", "0"], ["-1", "0"], ["NaN", "1"], ["1", "NaN"], ["NaN", "NaN"],
  ["Infinity", "-Infinity"], ["Infinity", "Infinity"], ["-Infinity", "0"], ["5e-324", "2"], ["1.7976931348623157e308", "1.7976931348623157e308"],
  ["0.1", "0.2"], ["1e308", "10"], ["2.5", "-0.5"], ["-0", "NaN"], ["NaN", "-0"], ["5e-324", "5e-324"], ["3", "2"], ["-7.5", "2"]];
const F32_PAIRS = [["0", "0"], ["-0", "0"], ["0", "-0"], ["-0", "-0"], ["1", "-1"], ["1", "0"], ["-1", "0"], ["NaN", "1"], ["1", "NaN"], ["NaN", "NaN"],
  ["Infinity", "-Infinity"], ["Infinity", "Infinity"], ["-Infinity", "0"], ["1e-45", "2"], ["3.4028235e38", "3.4028235e38"], ["0.1", "0.2"],
  ["16777216", "1"], ["2.5", "-0.5"], ["-0", "NaN"], ["NaN", "-0"], ["1e-45", "1e-45"], ["3", "2"], ["-7.5", "2"]];

// ---------- inteiros ----------
const intOps = (t) => {
  const bin = ["add", "sub", "mul", "div_s", "div_u", "rem_s", "rem_u", "and", "or", "xor", "shl", "shr_s", "shr_u", "rotl", "rotr"];
  const cmp = ["eq", "ne", "lt_s", "lt_u", "gt_s", "gt_u", "le_s", "le_u", "ge_s", "ge_u"];
  const un = t === "i32" ? ["clz", "ctz", "popcnt", "extend8_s", "extend16_s"] : ["clz", "ctz", "popcnt", "extend8_s", "extend16_s", "extend32_s"];
  let wat = "";
  for (const op of bin) wat += ` (func (export "${t}_${op}") (param ${t} ${t}) (result ${t}) (${t}.${op} (local.get 0) (local.get 1)))\n`;
  for (const op of cmp) wat += ` (func (export "${t}_${op}") (param ${t} ${t}) (result i32) (${t}.${op} (local.get 0) (local.get 1)))\n`;
  for (const op of un) wat += ` (func (export "${t}_${op}") (param ${t}) (result ${t}) (${t}.${op} (local.get 0)))\n`;
  wat += ` (func (export "${t}_eqz") (param ${t}) (result i32) (${t}.eqz (local.get 0)))\n`;
  return { wat: `(module\n${wat})`, bin, cmp, un };
};
for (const [t, pairs, unvals, chunk] of [["i32", I32_PAIRS, I32, 3], ["i64", I64_PAIRS, I64, 3]]) {
  const m = intOps(t);
  const prefix = prefixOf(m.wat);
  for (const op of [...m.bin, ...m.cmp]) calls(prefix, `${t}_${op}`, pairs, op.startsWith("div") || op.startsWith("rem") ? 4 : chunk + 4);
  for (const op of [...m.un, "eqz"]) calls(prefix, `${t}_${op}`, one(unvals), 8);
}

// ---------- ponto flutuante ----------
const floatWat = () => {
  let wat = "";
  for (const t of ["f32", "f64"]) {
    const it = t === "f32" ? "i32" : "i64";
    for (const op of ["add", "sub", "mul", "div", "min", "max", "copysign"]) {
      wat += ` (func (export "${t}_${op}") (param ${t} ${t}) (result ${t}) (${t}.${op} (local.get 0) (local.get 1)))\n`;
      wat += ` (func (export "${t}_${op}_bits") (param ${t} ${t}) (result ${it}) (${it}.reinterpret_${t} (${t}.${op} (local.get 0) (local.get 1))))\n`;
    }
    for (const op of ["eq", "ne", "lt", "gt", "le", "ge"]) wat += ` (func (export "${t}_${op}") (param ${t} ${t}) (result i32) (${t}.${op} (local.get 0) (local.get 1)))\n`;
    for (const op of ["abs", "neg", "ceil", "floor", "trunc", "nearest", "sqrt"]) {
      wat += ` (func (export "${t}_${op}") (param ${t}) (result ${t}) (${t}.${op} (local.get 0)))\n`;
      wat += ` (func (export "${t}_${op}_bits") (param ${t}) (result ${it}) (${it}.reinterpret_${t} (${t}.${op} (local.get 0))))\n`;
    }
    wat += ` (func (export "${t}_pass_bits") (param ${t}) (result ${it}) (${it}.reinterpret_${t} (local.get 0)))\n`;
    wat += ` (func (export "${t}_nan_const_bits") (result ${it}) (${it}.reinterpret_${t} (${t}.const nan)))\n`;
    wat += ` (func (export "${t}_nan_payload_bits") (result ${it}) (${it}.reinterpret_${t} (${t}.const nan:${t === "f32" ? "0x200001" : "0x8000000000001"})))\n`;
    wat += ` (func (export "${t}_nan_payload_add") (result ${it}) (${it}.reinterpret_${t} (${t}.add (${t}.const nan:${t === "f32" ? "0x200001" : "0x8000000000001"}) (${t}.const 1))))\n`;
    wat += ` (func (export "${t}_zero_div_zero_bits") (param ${t}) (result ${it}) (${it}.reinterpret_${t} (${t}.div (local.get 0) (local.get 0))))\n`;
    wat += ` (func (export "${t}_inf_sub_inf_bits") (param ${t}) (result ${it}) (${it}.reinterpret_${t} (${t}.sub (local.get 0) (local.get 0))))\n`;
  }
  return `(module\n${wat})`;
};
{
  const prefix = prefixOf(floatWat());
  for (const [t, pairs, un, bitPairs] of [["f32", F32_PAIRS, F32_UN, F32_PAIRS.slice(0, 20)], ["f64", F64_PAIRS, F64_UN, F64_PAIRS.slice(0, 20)]]) {
    for (const op of ["add", "sub", "mul", "div", "min", "max", "copysign"]) {
      calls(prefix, `${t}_${op}`, pairs, 6);
      calls(prefix, `${t}_${op}_bits`, bitPairs, 10);
    }
    for (const op of ["eq", "ne", "lt", "gt", "le", "ge"]) calls(prefix, `${t}_${op}`, pairs, 12);
    for (const op of ["abs", "neg", "ceil", "floor", "trunc", "nearest", "sqrt"]) {
      calls(prefix, `${t}_${op}`, one(un), 8);
      calls(prefix, `${t}_${op}_bits`, one(["NaN", "-0", "-1", "Infinity", "-Infinity", "4"]), 6);
    }
    calls(prefix, `${t}_pass_bits`, one(["NaN", "-NaN", "0", "-0", "Infinity", "1", t === "f32" ? "1e-45" : "5e-324"]), 7);
    for (const f of ["nan_const_bits", "nan_payload_bits", "nan_payload_add"]) calls(prefix, `${t}_${f}`, [[]], 1);
    calls(prefix, `${t}_zero_div_zero_bits`, one(["0", "NaN", "Infinity"]), 3);
    calls(prefix, `${t}_inf_sub_inf_bits`, one(["Infinity", "NaN", "1"]), 3);
  }
}

// ---------- conversões ----------
const convWat = () => {
  const lines = [];
  const f = (name, p, r, body) => lines.push(` (func (export "${name}") (param ${p}) (result ${r}) ${body})`);
  f("i32_wrap_i64", "i64", "i32", "(i32.wrap_i64 (local.get 0))");
  f("i64_extend_i32_s", "i32", "i64", "(i64.extend_i32_s (local.get 0))");
  f("i64_extend_i32_u", "i32", "i64", "(i64.extend_i32_u (local.get 0))");
  for (const [it, ft] of [["i32", "f32"], ["i32", "f64"], ["i64", "f32"], ["i64", "f64"]]) {
    for (const sx of ["s", "u"]) {
      f(`${it}_trunc_${ft}_${sx}`, ft, it, `(${it}.trunc_${ft}_${sx} (local.get 0))`);
      f(`${it}_trunc_sat_${ft}_${sx}`, ft, it, `(${it}.trunc_sat_${ft}_${sx} (local.get 0))`);
      f(`${ft}_convert_${it}_${sx}`, it, ft, `(${ft}.convert_${it}_${sx} (local.get 0))`);
    }
  }
  f("f32_demote_f64", "f64", "f32", "(f32.demote_f64 (local.get 0))");
  f("f64_promote_f32", "f32", "f64", "(f64.promote_f32 (local.get 0))");
  f("f32_demote_bits", "f64", "i32", "(i32.reinterpret_f32 (f32.demote_f64 (local.get 0)))");
  f("f64_promote_bits", "f32", "i64", "(i64.reinterpret_f64 (f64.promote_f32 (local.get 0)))");
  f("i32_reinterpret_f32", "f32", "i32", "(i32.reinterpret_f32 (local.get 0))");
  f("f32_reinterpret_i32", "i32", "f32", "(f32.reinterpret_i32 (local.get 0))");
  f("i64_reinterpret_f64", "f64", "i64", "(i64.reinterpret_f64 (local.get 0))");
  f("f64_reinterpret_i64", "i64", "f64", "(f64.reinterpret_i64 (local.get 0))");
  f("f32_roundtrip_i32", "i32", "i32", "(i32.reinterpret_f32 (f32.reinterpret_i32 (local.get 0)))");
  f("f64_roundtrip_i64", "i64", "i64", "(i64.reinterpret_f64 (f64.reinterpret_i64 (local.get 0)))");
  return `(module\n${lines.join("\n")}\n)`;
};
{
  const prefix = prefixOf(convWat());
  const TRUNC = ["0", "-0", "0.9", "-0.9", "1.5", "-1.5", "-1", "2147483520", "2147483647", "2147483647.9", "2147483648", "-2147483648", "-2147483648.9",
    "-2147483649", "-2147483904", "4294967040", "4294967295", "4294967295.9", "4294967296", "1e10", "-1e10", "9223372036854775807", "9223372036854775808",
    "-9223372036854775808", "-9223372036854777856", "1.8446742974197924e19", "18446744073709549568", "18446744073709551616", "1e20", "NaN", "Infinity", "-Infinity"];
  for (const it of ["i32", "i64"]) for (const ft of ["f32", "f64"]) for (const sx of ["s", "u"]) {
    calls(prefix, `${it}_trunc_${ft}_${sx}`, one(TRUNC), 6);
    calls(prefix, `${it}_trunc_sat_${ft}_${sx}`, one(TRUNC), 8);
  }
  const CI32 = [0, 1, -1, 16777216, 16777217, 33554435, 2147483647, -2147483648, 4294967295, 2147483649, 1431655765];
  const CI64 = ["0n", "1n", "-1n", "16777217n", "2n ** 53n + 1n", "2n ** 53n + 3n", "2n ** 63n - 1n", "-(2n ** 63n)", "2n ** 64n - 1n", "2n ** 63n + 2n ** 39n + 1n",
    "0x7fffff4000000001n", "0xffffff8000000001n", "9007199254740993n", "-9007199254740993n"];
  for (const ft of ["f32", "f64"]) for (const sx of ["s", "u"]) {
    calls(prefix, `${ft}_convert_i32_${sx}`, one(CI32), 6);
    calls(prefix, `${ft}_convert_i64_${sx}`, one(CI64), 5);
  }
  calls(prefix, "i32_wrap_i64", one(CI64), 7);
  calls(prefix, "i64_extend_i32_s", one(CI32), 6);
  calls(prefix, "i64_extend_i32_u", one(CI32), 6);
  calls(prefix, "f32_demote_f64", one(["1.5", "3.4028235e38", "3.4028235677973362e38", "3.4028235677973366e38", "1e39", "-1e39", "1e-46", "7e-46", "5e-324", "0.1", "-0", "NaN",
    "Infinity", "16777217", "1.1754943508222875e-38"]), 5);
  calls(prefix, "f64_promote_f32", one(["0.1", "-0", "NaN", "Infinity", "1e-45", "3.4028235e38", "16777217", "-Infinity"]), 8);
  calls(prefix, "f32_demote_bits", one(["NaN", "-0", "1e39", "Infinity"]), 4);
  calls(prefix, "f64_promote_bits", one(["NaN", "-0", "Infinity", "1e-45"]), 4);
  const BITS32 = [0, 1, -1, 2143289344, 2141192192, 2139095040, -8388608, -2147483648, 2147483647, 8388608, 8388607, 1065353216];
  calls(prefix, "i32_reinterpret_f32", one(["0", "-0", "NaN", "1", "-1", "Infinity", "1e-45", "3.4028235e38"]), 8);
  calls(prefix, "f32_reinterpret_i32", one(BITS32), 6);
  calls(prefix, "f32_roundtrip_i32", one(BITS32), 6);
  calls(prefix, "i64_reinterpret_f64", one(["0", "-0", "NaN", "1", "-1", "Infinity", "5e-324", "1.7976931348623157e308"]), 8);
  const BITS64 = ["0n", "1n", "-1n", "0x7ff8000000000000n", "0x7ff0000000000001n", "0x7ff0000000000000n", "0xfff0000000000000n", "0x8000000000000000n", "0x7fffffffffffffffn",
    "0x0010000000000000n", "0x000fffffffffffffn", "0x3ff0000000000000n"];
  calls(prefix, "f64_reinterpret_i64", one(BITS64), 6);
  calls(prefix, "f64_roundtrip_i64", one(BITS64), 6);
  // NaN observado do lado do JS: bits depois de passar por Float32Array/Float64Array.
  for (const bits of [2143289344, 2141192192, -1, 2139095041]) {
    add(prefix + `var f = x.f32_reinterpret_i32(${bits}); var a = new Float32Array(1); a[0] = f; L(String(f)); L(new Uint32Array(a.buffer)[0].toString(16)); L(x.i32_reinterpret_f32(f))`);
  }
  for (const bits of ["0x7ff8000000000000n", "0x7ff0000000000001n", "0xffffffffffffffffn"]) {
    add(prefix + `var f = x.f64_reinterpret_i64(${bits}); var a = new Float64Array(1); a[0] = f; L(String(f)); L(new BigUint64Array(a.buffer)[0].toString(16)); L(String(x.i64_reinterpret_f64(f)))`);
  }
  add(prefix + "var a = new Uint32Array([0x7fa00000]); var f = new Float32Array(a.buffer)[0]; L(x.i32_reinterpret_f32(f)); L(String(f))");
  add(prefix + "var a = new BigUint64Array([0x7ff4000000000000n]); var f = new Float64Array(a.buffer)[0]; L(String(x.i64_reinterpret_f64(f)))");
  add(prefix + "L(Z(() => x.i32_wrap_i64(1))); L(Z(() => x.i64_extend_i32_s(1n))); L(Z(() => x.f32_convert_i64_s(1))); L(Z(() => x.i32_trunc_f32_s(1n)))");
}

// ---------- memória ----------
{
  const dataBytes = "\\00\\01\\02\\03\\04\\05\\06\\07\\80\\81\\fe\\ff\\00\\00\\c0\\7f\\00\\00\\00\\00\\00\\00\\f8\\7f\\ff\\ff\\ff\\ff\\ff\\ff\\ff\\ff";
  const lines = [];
  for (const [t, kinds] of [["i32", ["", "8_s", "8_u", "16_s", "16_u"]], ["i64", ["", "8_s", "8_u", "16_s", "16_u", "32_s", "32_u"]], ["f32", [""]], ["f64", [""]]]) {
    for (const k of kinds) lines.push(` (func (export "${t}_load${k}") (param i32) (result ${t}) (${t}.load${k} (local.get 0)))`);
  }
  lines.push(` (func (export "i32_load_off") (param i32) (result i32) (i32.load offset=65532 (local.get 0)))`);
  lines.push(` (func (export "i32_load_offmax") (param i32) (result i32) (i32.load offset=4294967295 (local.get 0)))`);
  lines.push(` (func (export "i32_load_al1") (param i32) (result i32) (i32.load align=1 (local.get 0)))`);
  lines.push(` (func (export "i32_load_al2_off1") (param i32) (result i32) (i32.load offset=1 align=2 (local.get 0)))`);
  lines.push(` (func (export "i64_load_off8") (param i32) (result i64) (i64.load offset=8 (local.get 0)))`);
  lines.push(` (func (export "i64_load_al1") (param i32) (result i64) (i64.load align=1 (local.get 0)))`);
  lines.push(` (func (export "f64_load_al1_off3") (param i32) (result f64) (f64.load offset=3 align=1 (local.get 0)))`);
  for (const [t, kinds] of [["i32", ["", "8", "16"]], ["i64", ["", "8", "16", "32"]], ["f32", [""]], ["f64", [""]]]) {
    for (const k of kinds) lines.push(` (func (export "${t}_store${k}") (param i32 ${t}) (${t}.store${k} (local.get 0) (local.get 1)))`);
  }
  lines.push(` (func (export "i32_store_off") (param i32 i32) (i32.store offset=65532 (local.get 0) (local.get 1)))`);
  lines.push(` (func (export "i32_store_al1") (param i32 i32) (i32.store align=1 (local.get 0) (local.get 1)))`);
  lines.push(` (func (export "i64_store_off4") (param i32 i64) (i64.store offset=4 align=2 (local.get 0) (local.get 1)))`);
  lines.push(` (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0)))`);
  lines.push(` (func (export "size") (result i32) (memory.size))`);
  lines.push(` (func (export "fill") (param i32 i32 i32) (memory.fill (local.get 0) (local.get 1) (local.get 2)))`);
  lines.push(` (func (export "copy") (param i32 i32 i32) (memory.copy (local.get 0) (local.get 1) (local.get 2)))`);
  lines.push(` (func (export "init") (param i32 i32 i32) (memory.init $p (local.get 0) (local.get 1) (local.get 2)))`);
  lines.push(` (func (export "drop") (data.drop $p))`);
  lines.push(` (func (export "unreach") (unreachable))`);
  lines.push(` (func (export "load_after_grow") (param i32) (result i32) (drop (memory.grow (i32.const 1))) (i32.load (local.get 0)))`);
  const prefix = prefixOf(`(module\n (memory (export "mem") 1 2)\n (data (i32.const 0) "${dataBytes}")\n (data $p "\\aa\\bb\\cc\\dd")\n${lines.join("\n")}\n)`);
  const ADDRS = [0, 1, 3, 8, 12, 65528, 65529, 65532, 65533, 65535, 65536, 4294967295, -1, 2147483648, 1.5, "'8'"];
  for (const t of ["i32", "i64", "f32", "f64"]) {
    const kinds = t === "i32" ? ["", "8_s", "8_u", "16_s", "16_u"] : t === "i64" ? ["", "8_s", "8_u", "16_s", "16_u", "32_s", "32_u"] : [""];
    for (const k of kinds) calls(prefix, `${t}_load${k}`, one(ADDRS), 8);
  }
  for (const f of ["i32_load_off", "i32_load_offmax", "i32_load_al1", "i32_load_al2_off1", "i64_load_off8", "i64_load_al1", "f64_load_al1_off3"]) calls(prefix, f, one(ADDRS.slice(0, 13)), 7);
  const VALS = { i32: [0, -1, 305419896, 2147483648, 4294967295 + 2], i64: ["0n", "-1n", "0x0123456789abcdefn", "2n ** 63n", "2n ** 64n + 1n"], f32: ["1.5", "NaN", "-0", "Infinity", "1e-45"], f64: ["1.5", "NaN", "-0", "-Infinity", "5e-324"] };
  for (const t of ["i32", "i64", "f32", "f64"]) {
    const kinds = t === "i32" ? ["", "8", "16"] : t === "i64" ? ["", "8", "16", "32"] : [""];
    for (const k of kinds) {
      const items = [];
      for (const v of VALS[t]) items.push(`L(Z(() => x.${t}_store${k}(8, ${v})))`, `L(MB(x, 6, 20))`);
      items.push(`L(Z(() => x.${t}_store${k}(65535, ${VALS[t][0]})))`, `L(MB(x, 65528, 65536))`, `L(Z(() => x.${t}_store${k}(65536, ${VALS[t][1]})))`);
      emit(prefix, items, 12);
    }
  }
  emit(prefix, [`L(Z(() => x.i32_store(65532, -1)))`, `L(MB(x, 65530, 65536))`, `L(Z(() => x.i32_store(65533, 0x01020304)))`, `L(MB(x, 65530, 65536))`, `L(Z(() => x.i64_store(65529, 0x1122334455667788n)))`, `L(MB(x, 65526, 65536))`], 6);
  emit(prefix, [`L(Z(() => x.i32_store_off(0, 7)))`, `L(MB(x, 65532, 65536))`, `L(Z(() => x.i32_store_off(1, 7)))`, `L(Z(() => x.i32_store_al1(1, 0x0a0b0c0d)))`, `L(MB(x, 0, 8))`, `L(Z(() => x.i64_store_off4(2, 0x1122334455667788n)))`, `L(MB(x, 4, 16))`], 7);
  add(prefix + "L(Z(() => x.size())); L(Z(() => x.grow(0))); L(Z(() => x.grow(1))); L(Z(() => x.size())); L(Z(() => x.grow(1))); L(Z(() => x.size())); L(x.mem.buffer.byteLength)");
  add(prefix + "var b = x.mem.buffer; L(b.byteLength); L(Z(() => x.grow(1))); L(b.byteLength); L(x.mem.buffer.byteLength); L(x.mem.buffer === b)");
  add(prefix + "var b = x.mem.buffer; L(Z(() => x.grow(0))); L(b.byteLength); L(x.mem.buffer === b)");
  add(prefix + "L(Z(() => x.grow(65536))); L(Z(() => x.grow(-1))); L(Z(() => x.grow(4294967295))); L(Z(() => x.grow(2))); L(Z(() => x.size()))");
  add(prefix + "L(Z(() => x.grow('1'))); L(Z(() => x.grow(1.9))); L(Z(() => x.grow(1n)))");
  add(prefix + "L(Z(() => x.load_after_grow(131068))); L(Z(() => x.load_after_grow(131072))); L(Z(() => x.size()))");
  add(prefix + "L(Z(() => x.i32_load(65532))); L(Z(() => x.grow(1))); L(Z(() => x.i32_load(65536))); L(Z(() => x.i32_load(131072)))");
  const fills = [[0, 171, 16], [4, 511, 4], [4, -1, 4], [65535, 1, 1], [65535, 1, 2], [65536, 1, 0], [65537, 1, 0], [0, 7, 65536], [1, 7, 65536], [0, 0, 4294967295], [65530, 9, 6]];
  emit(prefix, fills.flatMap((a) => [`L(Z(() => x.fill(${a})))`, `L(MB(x, 0, 12))`, `L(MB(x, 65528, 65536))`]), 9);
  const copies = [[4, 0, 8], [0, 4, 8], [0, 0, 0], [8, 8, 8], [65528, 0, 8], [65529, 0, 8], [0, 65529, 8], [65536, 0, 0], [65537, 0, 0], [0, 65537, 0], [2, 0, 12], [0, 2, 12]];
  emit(prefix, copies.flatMap((a) => [`L(Z(() => x.copy(${a})))`, `L(MB(x, 0, 24))`, `L(MB(x, 65528, 65536))`]), 9);
  const inits = [[0, 0, 4], [0, 1, 4], [0, 1, 3], [10, 0, 4], [0, 0, 0], [0, 4, 0], [0, 5, 0], [65532, 0, 4], [65533, 0, 4], [65537, 0, 0], [0, 0, 5]];
  emit(prefix, inits.flatMap((a) => [`L(Z(() => x.init(${a})))`, `L(MB(x, 0, 16))`, `L(MB(x, 65528, 65536))`]), 9);
  add(prefix + "L(Z(() => x.drop())); L(Z(() => x.init(0, 0, 0))); L(Z(() => x.init(0, 0, 1))); L(Z(() => x.drop())); L(Z(() => x.init(65537, 0, 0)))");
  add(prefix + "L(Z(() => x.unreach())); L(Z(() => x.unreach()))");
  add(prefix + "var v = new DataView(x.mem.buffer); L(v.getUint32(8, true)); L(v.getFloat32(12, true)); L(v.getFloat64(16, true)); L(String(v.getBigUint64(24, true)))");
  add(prefix + "L(String(x.i64_load(8))); L(String(x.i64_load8_s(10))); L(String(x.i64_load16_u(10))); L(String(x.i64_load32_s(8))); L(String(x.i64_load32_u(8)))");
  add(prefix + "L(x.f32_load(12)); L(Object.is(x.f64_load(16), NaN)); L(x.i32_load(12)); L(x.f32_load(14)); L(x.i32_load8_s(11)); L(x.i32_load16_s(10))");
}

// ---------- SIMD ----------
const POOL = {
  i8: ["0", "1", "-1", "127", "-128", "255", "128", "2", "64", "-64", "100", "-100", "5", "250", "7", "9"],
  i16: ["0", "1", "-1", "32767", "-32768", "65535", "32768", "2", "16384", "-16384", "255", "-255", "5", "65530", "7", "9"],
  i32: ["0", "1", "-1", "2147483647", "-2147483648", "4294967295", "2147483648", "2", "65535", "65536", "-65536", "255", "5", "7", "100000", "-100000"],
  i64: ["0n", "1n", "-1n", "2n ** 63n - 1n", "-(2n ** 63n)", "2n ** 64n - 1n", "2n ** 63n", "2n", "4294967296n", "4294967295n", "-4294967296n", "255n", "5n", "7n", "1n << 40n", "-(1n << 40n)"],
  f32: ["0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "1.5", "2.5", "-2.5", "1e-45", "3.4028235e38", "0.5", "-0.5", "16777217", "5", "3e9", "-3e9", "4294967296", "255.9"],
  f64: ["0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "1.5", "2.5", "-2.5", "5e-324", "1.7976931348623157e308", "0.5", "-0.5", "9007199254740993", "5", "3e9", "-3e9", "4294967296", "1e20"],
};
const laneBits = { i8: 8, i16: 16, i32: 32, i64: 64, f32: 32, f64: 64 };
const vec = (type, c, which) => {
  const pool = POOL[type], n = 128 / laneBits[type];
  const lanes = [];
  for (let i = 0; i < n; i++) lanes.push(pool[(c * 5 + i * 3 + which * (c * 3 + 7) + (which ? 1 : 0)) % pool.length]);
  return `[${lanes.join(",")}]`;
};
const simdOps = []; // [shape, op, kind, in, out]
const S = (shape, ops, kind, it, ot) => ops.forEach((op) => simdOps.push([shape, op, kind, it, ot]));
const cmpS = ["eq", "ne", "lt_s", "lt_u", "gt_s", "gt_u", "le_s", "le_u", "ge_s", "ge_u"];
const cmpF = ["eq", "ne", "lt", "gt", "le", "ge"];
S("i8x16", ["add", "sub", "add_sat_s", "add_sat_u", "sub_sat_s", "sub_sat_u", "min_s", "min_u", "max_s", "max_u", "avgr_u", ...cmpS, "swizzle"], "b", "i8", "i8");
S("i8x16", ["narrow_i16x8_s", "narrow_i16x8_u"], "b", "i16", "i8");
S("i8x16", ["abs", "neg", "popcnt"], "u", "i8", "i8");
S("i8x16", ["shl", "shr_s", "shr_u"], "sh", "i8", "i8");
S("i8x16", ["all_true", "bitmask"], "s", "i8", "s");
S("i16x8", ["add", "sub", "mul", "add_sat_s", "add_sat_u", "sub_sat_s", "sub_sat_u", "min_s", "min_u", "max_s", "max_u", "avgr_u", "q15mulr_sat_s", ...cmpS], "b", "i16", "i16");
S("i16x8", ["narrow_i32x4_s", "narrow_i32x4_u"], "b", "i32", "i16");
S("i16x8", ["extmul_low_i8x16_s", "extmul_high_i8x16_s", "extmul_low_i8x16_u", "extmul_high_i8x16_u"], "b", "i8", "i16");
S("i16x8", ["extadd_pairwise_i8x16_s", "extadd_pairwise_i8x16_u", "extend_low_i8x16_s", "extend_high_i8x16_s", "extend_low_i8x16_u", "extend_high_i8x16_u"], "u", "i8", "i16");
S("i16x8", ["abs", "neg"], "u", "i16", "i16");
S("i16x8", ["shl", "shr_s", "shr_u"], "sh", "i16", "i16");
S("i16x8", ["all_true", "bitmask"], "s", "i16", "s");
S("i32x4", ["add", "sub", "mul", "min_s", "min_u", "max_s", "max_u", ...cmpS], "b", "i32", "i32");
S("i32x4", ["dot_i16x8_s", "extmul_low_i16x8_s", "extmul_high_i16x8_s", "extmul_low_i16x8_u", "extmul_high_i16x8_u"], "b", "i16", "i32");
S("i32x4", ["extadd_pairwise_i16x8_s", "extadd_pairwise_i16x8_u", "extend_low_i16x8_s", "extend_high_i16x8_s", "extend_low_i16x8_u", "extend_high_i16x8_u"], "u", "i16", "i32");
S("i32x4", ["abs", "neg"], "u", "i32", "i32");
S("i32x4", ["trunc_sat_f32x4_s", "trunc_sat_f32x4_u"], "u", "f32", "i32");
S("i32x4", ["trunc_sat_f64x2_s_zero", "trunc_sat_f64x2_u_zero"], "u", "f64", "i32");
S("i32x4", ["shl", "shr_s", "shr_u"], "sh", "i32", "i32");
S("i32x4", ["all_true", "bitmask"], "s", "i32", "s");
S("i64x2", ["add", "sub", "mul", "eq", "ne", "lt_s", "gt_s", "le_s", "ge_s"], "b", "i64", "i64");
S("i64x2", ["extmul_low_i32x4_s", "extmul_high_i32x4_s", "extmul_low_i32x4_u", "extmul_high_i32x4_u"], "b", "i32", "i64");
S("i64x2", ["extend_low_i32x4_s", "extend_high_i32x4_s", "extend_low_i32x4_u", "extend_high_i32x4_u"], "u", "i32", "i64");
S("i64x2", ["abs", "neg"], "u", "i64", "i64");
S("i64x2", ["shl", "shr_s", "shr_u"], "sh", "i64", "i64");
S("i64x2", ["all_true", "bitmask"], "s", "i64", "s");
for (const [shape, ft] of [["f32x4", "f32"], ["f64x2", "f64"]]) {
  S(shape, ["add", "sub", "mul", "div", "min", "max", "pmin", "pmax", ...cmpF], "b", ft, ft === "f32" ? "f32" : "f64");
  S(shape, ["abs", "neg", "sqrt", "ceil", "floor", "trunc", "nearest"], "u", ft, ft);
}
// as comparações devolvem máscaras inteiras
simdOps.forEach((o) => { if ((o[0] === "f32x4" || o[0] === "f64x2") && cmpF.includes(o[1])) o[4] = o[0] === "f32x4" ? "i32" : "i64"; });
S("f32x4", ["convert_i32x4_s", "convert_i32x4_u"], "u", "i32", "f32");
S("f32x4", ["demote_f64x2_zero"], "u", "f64", "f32");
S("f64x2", ["convert_low_i32x4_s", "convert_low_i32x4_u"], "u", "i32", "f64");
S("f64x2", ["promote_low_f32x4"], "u", "f32", "f64");
S("v128", ["and", "or", "xor", "andnot"], "b", "i32", "i32");
S("v128", ["not"], "u", "i32", "i32");
S("v128", ["bitselect"], "t", "i32", "i32");
S("v128", ["any_true"], "s", "i32", "s");
// any_true de v128 é a única instrução escalar do grupo; o nome da função fica v128_any_true
{
  const fnName = (o) => `${o[0]}_${o[1]}`;
  const ld = (off) => `(v128.load (i32.const ${off}))`;
  const body = (o) => {
    const [shape, op, kind] = o;
    const full = `${shape}.${op}`;
    if (kind === "b") return `(func (export "${fnName(o)}") (v128.store (i32.const 64) (${full} ${ld(0)} ${ld(16)})))`;
    if (kind === "u") return `(func (export "${fnName(o)}") (v128.store (i32.const 64) (${full} ${ld(0)})))`;
    if (kind === "t") return `(func (export "${fnName(o)}") (v128.store (i32.const 64) (${full} ${ld(0)} ${ld(16)} ${ld(32)})))`;
    if (kind === "sh") return `(func (export "${fnName(o)}") (param i32) (v128.store (i32.const 64) (${full} ${ld(0)} (local.get 0))))`;
    return `(func (export "${fnName(o)}") (result i32) (${full} ${ld(0)}))`;
  };
  const shuffles = [
    "0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15", "16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31", "15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0",
    "0 16 1 17 2 18 3 19 4 20 5 21 6 22 7 23", "31 0 30 1 29 2 28 3 27 4 26 5 25 6 24 7", "0 0 0 0 16 16 16 16 5 5 5 5 31 31 31 31",
  ];
  const shuffleFns = shuffles.map((s, i) => `(func (export "shuffle${i}") (v128.store (i32.const 64) (i8x16.shuffle ${s} ${ld(0)} ${ld(16)})))`);
  const lane = [];
  const lf = (name, params, res, code) => lane.push(`(func (export "${name}") ${params} ${res} ${code})`);
  const st = (e) => `(v128.store (i32.const 64) ${e})`;
  lf("i8x16_splat", "(param i32)", "", st("(i8x16.splat (local.get 0))"));
  lf("i16x8_splat", "(param i32)", "", st("(i16x8.splat (local.get 0))"));
  lf("i32x4_splat", "(param i32)", "", st("(i32x4.splat (local.get 0))"));
  lf("i64x2_splat", "(param i64)", "", st("(i64x2.splat (local.get 0))"));
  lf("f32x4_splat", "(param f32)", "", st("(f32x4.splat (local.get 0))"));
  lf("f64x2_splat", "(param f64)", "", st("(f64x2.splat (local.get 0))"));
  for (const k of [0, 5, 15]) {
    lf(`i8x16_extract_s${k}`, "", "(result i32)", `(i8x16.extract_lane_s ${k} ${ld(0)})`);
    lf(`i8x16_extract_u${k}`, "", "(result i32)", `(i8x16.extract_lane_u ${k} ${ld(0)})`);
    lf(`i8x16_replace${k}`, "(param i32)", "", st(`(i8x16.replace_lane ${k} ${ld(0)} (local.get 0))`));
  }
  for (const k of [0, 7]) {
    lf(`i16x8_extract_s${k}`, "", "(result i32)", `(i16x8.extract_lane_s ${k} ${ld(0)})`);
    lf(`i16x8_extract_u${k}`, "", "(result i32)", `(i16x8.extract_lane_u ${k} ${ld(0)})`);
    lf(`i16x8_replace${k}`, "(param i32)", "", st(`(i16x8.replace_lane ${k} ${ld(0)} (local.get 0))`));
  }
  lf("i32x4_extract2", "", "(result i32)", `(i32x4.extract_lane 2 ${ld(0)})`);
  lf("i32x4_replace3", "(param i32)", "", st(`(i32x4.replace_lane 3 ${ld(0)} (local.get 0))`));
  lf("i64x2_extract1", "", "(result i64)", `(i64x2.extract_lane 1 ${ld(0)})`);
  lf("i64x2_replace0", "(param i64)", "", st(`(i64x2.replace_lane 0 ${ld(0)} (local.get 0))`));
  lf("f32x4_extract3", "", "(result f32)", `(f32x4.extract_lane 3 ${ld(0)})`);
  lf("f32x4_replace1", "(param f32)", "", st(`(f32x4.replace_lane 1 ${ld(0)} (local.get 0))`));
  lf("f64x2_extract1", "", "(result f64)", `(f64x2.extract_lane 1 ${ld(0)})`);
  lf("f64x2_replace1", "(param f64)", "", st(`(f64x2.replace_lane 1 ${ld(0)} (local.get 0))`));
  lf("const", "", "", st("(v128.const i32x4 0x01020304 0x80000000 0xffffffff 0)"));
  lf("const_f", "", "", st("(v128.const f32x4 nan -0 inf 1.5)"));
  const loadKinds = ["v128.load", "v128.load8_splat", "v128.load16_splat", "v128.load32_splat", "v128.load64_splat", "v128.load8x8_s", "v128.load8x8_u", "v128.load16x4_s",
    "v128.load16x4_u", "v128.load32x2_s", "v128.load32x2_u", "v128.load32_zero", "v128.load64_zero"];
  for (const k of loadKinds) lf(k.replace(".", "_"), "(param i32)", "", st(`(${k} (local.get 0))`));
  lf("load8_lane", "(param i32)", "", st(`(v128.load8_lane 3 (local.get 0) ${ld(0)})`));
  lf("load16_lane", "(param i32)", "", st(`(v128.load16_lane 2 (local.get 0) ${ld(0)})`));
  lf("load32_lane", "(param i32)", "", st(`(v128.load32_lane 1 (local.get 0) ${ld(0)})`));
  lf("load64_lane", "(param i32)", "", st(`(v128.load64_lane 1 (local.get 0) ${ld(0)})`));
  lf("store8_lane", "(param i32)", "", `(v128.store8_lane 5 (local.get 0) ${ld(0)})`);
  lf("store16_lane", "(param i32)", "", `(v128.store16_lane 3 (local.get 0) ${ld(0)})`);
  lf("store32_lane", "(param i32)", "", `(v128.store32_lane 2 (local.get 0) ${ld(0)})`);
  lf("store64_lane", "(param i32)", "", `(v128.store64_lane 1 (local.get 0) ${ld(0)})`);
  lf("load_off", "(param i32)", "", st("(v128.load offset=65520 (local.get 0))"));
  lf("store_v", "(param i32)", "", `(v128.store (local.get 0) ${ld(0)})`);
  const wat = `(module\n (memory (export "mem") 1)\n (data (i32.const 128) "\\00\\01\\02\\03\\04\\05\\06\\07\\80\\81\\82\\83\\84\\85\\86\\ff\\11\\22\\33\\44")\n ${simdOps.map(body).join("\n ")}\n ${shuffleFns.join("\n ")}\n ${lane.join("\n ")}\n)`;
  const prefix = prefixOf(wat);
  const SHIFTS = [1, 7, 8, 9, 15, 16, 31, 32, 33, 63, 64, 65, -1, 4294967295];
  for (const o of simdOps) {
    const [, , kind, it, ot] = o;
    const arity = kind === "t" ? 3 : kind === "b" ? 2 : 1;
    const nCases = kind === "s" || kind === "u" ? 4 : 3;
    const items = [];
    for (let c = 0; c < nCases; c++) {
      const vecs = [];
      for (let w = 0; w < arity; w++) vecs.push(vec(it, c, w));
      const args = kind === "sh" ? `, [${SHIFTS[(c * 5 + o[1].length) % SHIFTS.length]}]` : "";
      items.push(`L(SV(x, '${fnName(o)}', '${it}', '${ot}', [${vecs.join(",")}]${args}))`);
    }
    emit(prefix, items, nCases);
  }
  const shuffleIdx = [0, 1, 2, 3, 4, 5];
  for (const i of shuffleIdx) emit(prefix, [0, 1].map((c) => `L(SV(x, 'shuffle${i}', 'i8', 'i8', [${vec("i8", c, 0)},${vec("i8", c, 1)}]))`), 2);
  emit(prefix, [0, 1, 2].map((c) => `L(SV(x, 'i8x16_swizzle', 'i8', 'i8', [${vec("i8", c, 0)},[0,15,16,255,1,17,-1,128,3,2,7,8,31,32,14,100]]))`), 3);
  // lanes e splats
  const splat = (name, it, vals) => emit(prefix, vals.map((v) => `L(SV(x, '${name}', '${it}', '${it}', [], [${v}]))`), vals.length);
  splat("i8x16_splat", "i8", [0, -1, 255, 256, 127, 1.5]);
  splat("i16x8_splat", "i16", [0, -1, 65535, 65536, 32768]);
  splat("i32x4_splat", "i32", [0, -1, 4294967295, 2147483648, "'5'"]);
  splat("i64x2_splat", "i64", ["0n", "-1n", "2n ** 64n - 1n", "2n ** 63n"]);
  splat("f32x4_splat", "f32", ["0", "-0", "NaN", "1e-45", "3.4028235e38", "Infinity"]);
  splat("f64x2_splat", "f64", ["0", "-0", "NaN", "5e-324", "1.7976931348623157e308", "-Infinity"]);
  add(prefix + "L(Z(() => x.i64x2_splat(1))); L(Z(() => x.i32x4_splat(1n))); L(Z(() => x.f32x4_splat('2.5')))");
  const lanesOf = (it, c) => `[${vec(it, c, 0)}]`;
  for (const [name, it, ot] of [["i8x16_extract_s0", "i8", "s"], ["i8x16_extract_u0", "i8", "s"], ["i8x16_extract_s5", "i8", "s"], ["i8x16_extract_u5", "i8", "s"], ["i8x16_extract_s15", "i8", "s"],
    ["i8x16_extract_u15", "i8", "s"], ["i16x8_extract_s0", "i16", "s"], ["i16x8_extract_u0", "i16", "s"], ["i16x8_extract_s7", "i16", "s"], ["i16x8_extract_u7", "i16", "s"],
    ["i32x4_extract2", "i32", "s"], ["i64x2_extract1", "i64", "s"], ["f32x4_extract3", "f32", "s"], ["f64x2_extract1", "f64", "s"]]) {
    emit(prefix, [0, 1, 2, 3].map((c) => `L(SV(x, '${name}', '${it}', '${ot}', [${lanesOf(it, c)}]))`), 4);
  }
  for (const [name, it, vals] of [["i8x16_replace0", "i8", [1, -1, 300]], ["i8x16_replace5", "i8", [255, 128]], ["i8x16_replace15", "i8", [7, -129]], ["i16x8_replace0", "i16", [1, 70000]],
    ["i16x8_replace7", "i16", [-1, 32768]], ["i32x4_replace3", "i32", [0, -1, 4294967295]], ["i64x2_replace0", "i64", ["5n", "-1n"]], ["f32x4_replace1", "f32", ["NaN", "-0", "1e-45"]],
    ["f64x2_replace1", "f64", ["NaN", "-0", "5e-324"]]]) {
    emit(prefix, vals.map((v, c) => `L(SV(x, '${name}', '${it}', '${it}', [${lanesOf(it, c)}], [${v}]))`), vals.length);
  }
  add(prefix + "L(SV(x, 'const', 'i32', 'i32', [])); L(SV(x, 'const_f', 'f32', 'f32', []))");
  // cargas e armazenamentos de v128
  const LADDR = [128, 129, 133, 136, 140, 65528, 65520, 65521, 65535, 65536, -1, 4294967295, 0];
  for (const k of loadKinds) {
    const fn = k.replace(".", "_");
    emit(prefix, LADDR.map((a) => `L(SV(x, '${fn}', 'i8', 'i8', [], [${a}]))`), 7);
  }
  for (const [fn, it] of [["load8_lane", "i8"], ["load16_lane", "i16"], ["load32_lane", "i32"], ["load64_lane", "i64"]]) {
    emit(prefix, [128, 135, 65535, 65534, 65528, 65536].map((a) => `L(SV(x, '${fn}', '${it}', '${it}', [${lanesOf(it, 1)}], [${a}]))`), 6);
  }
  for (const fn of ["store8_lane", "store16_lane", "store32_lane", "store64_lane"]) {
    emit(prefix, [200, 65535, 65534, 65532, 65528, 65536].map((a) => `L(SV(x, '${fn}', 'i8', 'i8', [${lanesOf("i8", 2)}], [${a}])); L(MB(x, ${a === 200 ? 200 : 65526}, ${a === 200 ? 216 : 65536}))`), 6);
  }
  emit(prefix, [0, 8, 65520, 65521, 4294967295].map((a) => `L(SV(x, 'load_off', 'i8', 'i8', [${lanesOf("i8", 0)}], [${a}]))`), 5);
  emit(prefix, [300, 65520, 65521, 65535].map((a) => `L(SV(x, 'store_v', 'i8', 'i8', [${lanesOf("i8", 3)}], [${a}])); L(MB(x, 65520, 65536))`), 4);
  add(prefix + "L(Z(() => x.i8x16_add(1))); L(Z(() => x.i32x4_shl())); L(Z(() => x.i32x4_shl(1n))); L(typeof x.i32x4_add)");
}
// relaxed SIMD (módulos separados: se o motor não tem a extensão, o golden registra o CompileError)
{
  const rel = [
    ["i8x16.relaxed_swizzle", "b", "i8", "i8"], ["i32x4.relaxed_trunc_f32x4_s", "u", "f32", "i32"], ["i32x4.relaxed_trunc_f32x4_u", "u", "f32", "i32"],
    ["f32x4.relaxed_madd", "t", "f32", "f32"], ["f32x4.relaxed_nmadd", "t", "f32", "f32"], ["f64x2.relaxed_madd", "t", "f64", "f64"],
    ["f32x4.relaxed_min", "b", "f32", "f32"], ["f32x4.relaxed_max", "b", "f32", "f32"], ["i8x16.relaxed_laneselect", "t", "i8", "i8"],
    ["i16x8.relaxed_q15mulr_s", "b", "i16", "i16"], ["i16x8.relaxed_dot_i8x16_i7x16_s", "b", "i8", "i16"], ["i32x4.relaxed_dot_i8x16_i7x16_add_s", "t", "i8", "i32"],
  ];
  const ld = (off) => `(v128.load (i32.const ${off}))`;
  for (const [op, kind, it, ot] of rel) {
    const args = kind === "t" ? `${ld(0)} ${ld(16)} ${ld(32)}` : kind === "b" ? `${ld(0)} ${ld(16)}` : ld(0);
    const prefix = prefixOf(`(module (memory (export "mem") 1) (func (export "f") (v128.store (i32.const 64) (${op} ${args}))))`);
    const arity = kind === "t" ? 3 : kind === "b" ? 2 : 1;
    // entradas determinísticas: sem NaN nem fora de faixa nos truncamentos relaxados
    const items = [0, 1].map((c) => {
      const vecs = [];
      for (let w = 0; w < arity; w++) vecs.push(op.includes("trunc") ? "[1.5,-2.5,100,0]" : vec(it, c + 1, w).replace(/NaN|Infinity|-Infinity|3\.4028235e38|1\.7976931348623157e308/g, "7"));
      return `L(SV(x, 'f', '${it}', '${ot}', [${vecs.join(",")}]))`;
    });
    emit(prefix, items, 2);
  }
}

if (programs.length < 500) throw new Error("só " + programs.length + " programas");

const lines = [];
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script = harness + `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\n{ const out = __final(); process.stdout.write(out); }\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 20000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
