// Gera tests/golden/number_format_bun.tsv: formatação e leitura de Number, Math fora do libm, BigInt(number) e
// toLocaleString básico, medidos no bun 1.4.2. Colunas: a fonte do programa (JSON) e o valor da variável global
// `R` (JSON), no formato de gen-function-error-golden.js. Cada programa grava `R` como texto; exceções viram
// "Nome: mensagem" e -0 vira "-0". Os programas rodam por eval indireto num único processo.
// Uso: bun scripts/gen-number-format-golden.js > tests/golden/number_format_bun.tsv
const programs = [];
const seen = new Set();
const add = expr => {
  if (!seen.has(expr)) {
    seen.add(expr);
    programs.push(expr);
  }
};

// ---- Number.prototype.toString(radix).
const radixValues = [
  "0", "-0", "1", "-1", "0.5", "0.1", "255", "-255.75", "1e21", "1e-7", "123456789.123456789", "2**53", "2**-30",
  "1e300", "5e-324", "1.7976931348623157e308", "Math.PI", "-Math.E", "0.000001", "1/3", "NaN", "Infinity", "-Infinity",
  "2**64", "1e100", "0.30000000000000004", "4.35", "1e-10",
];
for (const radix of [2, 3, 7, 8, 10, 12, 16, 20, 32, 36]) {
  for (const value of radixValues) add(`(${value}).toString(${radix})`);
}
for (let radix = 2; radix <= 36; radix++) {
  for (const value of ["255", "0.1", "-12345.6789", "2**60", "1e21", "1/7"]) add(`(${value}).toString(${radix})`);
}
for (const radix of ["1", "37", "0", "'x'", "undefined", "2.9", "null", "NaN", "Infinity"]) add(`(10).toString(${radix})`);

// ---- toFixed, toExponential, toPrecision.
const fmtValues = [
  "0", "-0", "1.005", "0.5", "1.5", "2.5", "-2.5", "1e21", "1e20", "5e-324", "1.7976931348623157e308", "123.456",
  "0.000001", "1e-7", "0.1", "0.3", "1.45", "8.345", "10.235", "NaN", "Infinity", "-Infinity", "999.9995", "0.00001",
  "123456789012345680000",
];
for (let digits = 0; digits <= 100; digits++) {
  for (const value of ["1.005", "1/3", "5e-324", "1.7976931348623157e308"]) {
    add(`(${value}).toFixed(${digits})`);
    add(`(${value}).toExponential(${digits})`);
    if (digits >= 1) add(`(${value}).toPrecision(${digits})`);
  }
}
for (const digits of [0, 1, 2, 3, 5, 10, 20]) {
  for (const value of fmtValues) {
    add(`(${value}).toFixed(${digits})`);
    add(`(${value}).toExponential(${digits})`);
    add(`(${value}).toPrecision(${digits || 1})`);
  }
}
for (const value of fmtValues) {
  add(`(${value}).toExponential()`);
  add(`(${value}).toPrecision()`);
  add(`(${value}).toFixed()`);
}
for (const arg of ["-1", "101", "NaN", "undefined", "'3'", "1.9", "Infinity", "-0.5", "null"]) {
  add(`(1.5).toFixed(${arg})`);
  add(`(1.5).toExponential(${arg})`);
  add(`(1.5).toPrecision(${arg})`);
}
for (const arg of ["0", "101", "-1"]) add(`(1.5).toPrecision(${arg})`);

// ---- Number(), parseFloat e parseInt de strings.
const strings = [
  "", " ", "  12  ", "0x1F", "0X1f", "0o17", "0O17", "0b101", "0B101", "-0x10", "+0x10", "0x", "0b2", "0o8", "1_000",
  "1__0", "_1", "1_", "0x1_0", "1e1_0", "Infinity", "-Infinity", "+Infinity", "infinity", "INFINITY", "Inf", "NaN",
  "1e", "1e+", "1e-", ".5", "5.", ".", "-.5", "+.5e1", "1e1000", "-1e1000", "1e-1000", "1e308", "1.7976931348623158e308",
  "1.7976931348623159e308", "2.4703282292062327e-324", "2.4703282292062328e-324", "4.9e-324", "1e-323", "0e1000",
  "00012", "-0", "+0", "12abc", "1,5", " 12 ", " 1 ", "﻿7﻿", "　1　", "᠎1",
  "​1", "\n\t 3 \r\n", "1 2", "0.0000001", "123456789012345678901234567890", "9007199254740993", "9007199254740992.5",
  "0.1e1", "1E3", "1e+3", "1e-3", "--1", "+-1", "0x-1", "٣", "1e0x", "1n", "0.5e", "0e0", "-0e0", "00", "08", "010",
];
for (const text of strings) {
  const literal = JSON.stringify(text);
  add(`Number(${literal})`);
  add(`parseFloat(${literal})`);
  add(`parseInt(${literal})`);
  add(`+${literal}`);
}
for (const radix of [0, 2, 8, 10, 16, 36, 37, 1, -1, 32]) {
  for (const text of ["10", "0x10", "-0x10", "zz", "Zz", "12abc", "  7", "9007199254740993", "0b11", "1e3", "-", "0"]) {
    add(`parseInt(${JSON.stringify(text)}, ${radix})`);
  }
}
for (const text of ["1".repeat(400), "9".repeat(400), "0." + "0".repeat(400) + "1", "1" + "0".repeat(308), "1" + "0".repeat(309),
  "0." + "0".repeat(323) + "5", "0." + "0".repeat(323) + "25", "0." + "0".repeat(323) + "24", "1e" + "9".repeat(30),
  "1e-" + "9".repeat(30), "0e" + "9".repeat(30), "123456789".repeat(111) + "1", "0." + "3".repeat(1000)]) {
  add(`Number(${JSON.stringify(text)})`);
  add(`parseFloat(${JSON.stringify(text)})`);
}
// Meio-termo de arredondamento: 2**53 + 1, 2**53 + 3 e vizinhos, com dígitos extras desempatando.
for (const text of ["9007199254740993", "9007199254740993.0000000001", "9007199254740992.99999999999999999999",
  "9007199254740995", "9007199254740995.0000000000000001", "18014398509481985", "18014398509481985.000000000000001",
  "18014398509481984.999999999999999", "1.00000000000000011102230246251565404236316680908203125",
  "1.00000000000000011102230246251565404236316680908203126", "1.00000000000000011102230246251565404236316680908203124",
  "4.940656458412465441765687928682213723651e-324", "2.470328229206232720882843964341106861825e-324",
  "2.4703282292062327208828439643411068618252990130716238221279284125033775363510437593264991818081799618989828234772285886546332835517796989819938739800539093906315035659515570226392290858392449105184435931802849936536152500319370457678249219365623669863658480757001585769269903706311928279558551332927834338409351978015531246597263579574622766465272827220056374006485499977096599470454020828166226237857393450736339007967761930577506740176324673600968951340535537458516661134223766678604162159680461914467291840300530057530849048765391711386591646239524912623653881879636239373280423891018672348497668235089863388587925628302755995657524455507255189313690836254779186948667994968324049705821028513185451396213837722826145437693412532098591327667236328125e-324"]) {
  add(`Number(${JSON.stringify(text)})`);
}
for (let k = 53; k <= 63; k++) {
  add(`Number(String(2n**${k}n + 1n))`);
  add(`Number(String(2n**${k}n + 2n**${k - 53}n))`);
  add(`Number(String(2n**${k}n + 2n**${k - 53}n + 1n))`);
  add(`Number(String(2n**${k}n + 2n**${k - 53}n - 1n))`);
  add(`BigInt(2**${k})`);
}

// ---- String(number) nas fronteiras do shortest repr: potências de 2 e de 10, mais e menos 1 ulp.
const bits = `const b = new DataView(new ArrayBuffer(8));
const next = (v, d) => { b.setFloat64(0, v); b.setBigUint64(0, b.getBigUint64(0) + BigInt(d)); return b.getFloat64(0); };`;
for (let e = -1074; e <= 1023; e++) {
  // Todas as potências de 2 (em duas famílias de teste agrupadas para ficar abaixo de ~2500 programas no total).
  if (e % 3 === 0 || e > 1000 || e < -1070) {
    add(`${bits}\nconst v = 2 ** ${e};\nglobalThis.R = [String(next(v, -1)), String(v), String(next(v, 1))].join(' ')`);
  }
}
for (let e = -323; e <= 308; e++) {
  if (e % 5 === 0 || e > 300 || e < -320) {
    add(`${bits}\nconst v = 1e${e};\nglobalThis.R = [String(next(v, -1)), String(v), String(next(v, 1))].join(' ')`);
  }
}
for (const v of ["0.1", "0.2", "0.3", "1e21", "1e-6", "1e-7", "123456789012345680000", "999999999999999900000", "1e21 - 1e5",
  "2**53", "2**53 + 2", "0.000001", "0.0000001", "4.35", "0.1 + 0.2", "100", "1e2", "1.5e300", "-1e-7", "-1e21"]) {
  add(`String(${v})`);
  add(`(${v}) + ''`);
  add(`\`\${${v}}\``);
  add(`JSON.stringify(${v})`);
}

// ---- Constantes e predicados de Number.
for (const v of ["0", "-0", "1", "1.5", "NaN", "Infinity", "-Infinity", "2**53", "2**53 - 1", "-(2**53 - 1)", "2**53 + 2",
  "1e300", "5e-324", "'1'", "null", "undefined", "true", "[]", "1e21", "9007199254740991", "9007199254740992", "-9007199254740991.5"]) {
  add(`Number.isInteger(${v})`);
  add(`Number.isSafeInteger(${v})`);
  add(`Number.isFinite(${v})`);
  add(`Number.isNaN(${v})`);
  add(`isFinite(${v})`);
  add(`isNaN(${v})`);
}
for (const name of ["EPSILON", "MAX_SAFE_INTEGER", "MIN_SAFE_INTEGER", "MAX_VALUE", "MIN_VALUE", "POSITIVE_INFINITY", "NEGATIVE_INFINITY", "NaN"]) {
  add(`Number.${name}`);
  add(`1 + Number.${name} === 1`);
  add(`Object.getOwnPropertyDescriptor(Number, '${name}')`.replace(/^(.*)$/, "JSON.stringify($1)"));
}
add("Number.parseFloat === parseFloat");
add("Number.parseInt === parseInt");
add("Number.MAX_SAFE_INTEGER + 2");
add("Number.EPSILON === 2 ** -52");
add("Number.MIN_VALUE / 2");

// ---- BigInt(number), asIntN, asUintN.
for (const v of ["0", "-0", "1", "-1", "2**53", "2**53 + 2", "1e21", "1e300", "2**1023", "-(2**64)", "2**64", "123456789012345680000",
  "1.5", "0.1", "NaN", "Infinity", "-Infinity", "1e-7", "Number.MAX_VALUE", "Number.MIN_VALUE", "5e-324", "2**-1074", "-1e21", "'12'", "true"]) {
  add(`BigInt(${v})`);
}
for (const bitsCount of [0, 1, 2, 8, 31, 32, 33, 63, 64, 65, 100]) {
  for (const v of ["0n", "1n", "-1n", "127n", "128n", "255n", "256n", "-128n", "-129n", "2n**63n", "2n**64n", "-(2n**63n)", "-(2n**63n) - 1n",
    "2n**100n + 5n", "-(2n**100n) - 5n", "2n**200n - 1n"]) {
    add(`BigInt.asIntN(${bitsCount}, ${v})`);
    add(`BigInt.asUintN(${bitsCount}, ${v})`);
  }
}
for (const arg of ["-1", "2**53", "'3'", "1.9", "NaN", "undefined", "2**53 - 1", "-0"]) {
  add(`BigInt.asIntN(${arg}, 5n)`);
  add(`BigInt.asUintN(${arg}, 5n)`);
}
add("BigInt.asIntN(8, 1)");
add("BigInt.asUintN(8, '1')");

// ---- Math fora do libm.
const mathValues = [
  "0", "-0", "1", "-1", "0.5", "-0.5", "1.5", "-1.5", "2.5", "-2.5", "0.49999999999999994", "-0.49999999999999994",
  "2**52", "-(2**52)", "2**52 + 0.5", "2**52 + 1", "2**53", "2**53 + 2", "4503599627370495.5", "-4503599627370495.5",
  "4503599627370497", "Number.MAX_VALUE", "-Number.MAX_VALUE", "Number.MIN_VALUE", "-Number.MIN_VALUE", "NaN", "Infinity",
  "-Infinity", "1e-300", "1e300", "0.1", "-0.1", "3.9999999999999996", "1.0000000000000002", "123456.789", "-123456.789",
  "1e-10", "2**31", "2**32", "2**32 + 1", "-(2**31)", "2**31 - 1", "0.9999999999999999", "-0.9999999999999999", "27", "-27", "8", "1e21",
];
for (const v of mathValues) {
  for (const fn of ["fround", "clz32", "trunc", "sign", "cbrt", "expm1", "log1p", "round", "floor", "ceil", "abs", "sqrt", "hypot"]) {
    add(`Math.${fn}(${v})`);
  }
}
add("Math.round(0.49999999999999994)");
add("Math.round(-0.49999999999999994)");
add("Math.round(2**52)");
add("Math.round(-0)");
add("Math.round(-0.5)");
add("Math.round(2.5)");
add("Math.round(-2.5)");
add("Math.round(0.5)");
add("Math.fround(5.5)");
add("Math.fround(5.05)");
add("Math.fround(1e40)");
add("Math.fround(1e-50)");
add("Math.fround(3.4028235677973366e38)");
add("Math.fround(3.4028234663852886e38)");
add("Math.fround(3.4028235677973362e38)");
add("Math.fround(1.401298464324817e-45)");
add("Math.fround(7.006492321624085e-46)");
add("Math.fround(7.006492321624087e-46)");
add("Math.fround(16777217)");
add("Math.fround(16777219)");
const imulValues = ["0", "1", "-1", "2", "0xffffffff", "0x7fffffff", "0x80000000", "65536", "123456789", "-123456789", "1.9", "-1.9", "NaN", "Infinity", "2**53", "2**32 + 5", "-0", "'3'", "undefined"];
for (const x of imulValues) {
  for (const y of ["0", "5", "-5", "0xffffffff", "0x7fffffff", "0x80000000", "65537", "3.7", "NaN", "2**33 + 3"]) add(`Math.imul(${x}, ${y})`);
}
for (const x of ["1", "0", "-1", "0x80000000", "0x7fffffff", "0xffffffff", "2**32", "2**32 + 1", "0.5", "1e10", "-0", "NaN", "Infinity", "'8'", "undefined", "65535", "65536"]) add(`Math.clz32(${x})`);
add("Math.clz32()");
add("Math.imul()");
for (const args of ["", "3, 4", "3, 4, 12", "-0", "-0, 0", "NaN, 3", "Infinity, NaN", "-Infinity, NaN", "1e200, 1e200", "1e-200, 1e-200",
  "1e308, 1e308", "3, 4, NaN", "Infinity, 3", "5e-324, 5e-324", "0.1, 0.2, 0.3", "1, 2, 3, 4, 5, 6, 7, 8, 9, 10", "'3', '4'", "1e154, 1e154, 1e154",
  "-3, -4", "0, 0", "-0, -0", "0.30000000000000004, 0.1", "123456789, 987654321, 1e5"]) add(`Math.hypot(${args})`);
for (const args of ["", "1", "-0, 0", "0, -0", "-0, -0", "0, 0", "NaN, 1", "1, NaN", "1, 2, NaN", "-0, NaN", "Infinity, -Infinity", "-Infinity, Infinity",
  "1, '3'", "'a', 1", "undefined", "null, -1", "true, 0.5", "[2], [3]", "1, 2, 3, 4, 5", "-0, -1e-320", "5e-324, 0", "2**53, 2**53 + 2", "[], 0"]) {
  add(`Math.max(${args})`);
  add(`Math.min(${args})`);
}
for (const v of ["0", "-0", "1", "-1", "NaN", "Infinity", "-Infinity", "0.5", "-0.5", "1e-320", "-1e-320", "'5'", "'-5'", "null", "undefined", "true", "[]", "{}"]) {
  add(`Math.sign(${v})`);
  add(`Math.trunc(${v})`);
  add(`Math.cbrt(${v})`);
}
for (const v of ["1e-300", "1e-10", "1e-5", "0.5", "-0.5", "-0.9999999999999999", "-1", "-1.0000000000000002", "1e-17", "-1e-17", "40", "700", "710", "1e-320", "-745", "-1e300"]) {
  add(`Math.expm1(${v})`);
  add(`Math.log1p(${v})`);
}
add("Math.cbrt(27)");
add("Math.cbrt(-8)");
add("Math.cbrt(1e-300)");
add("Math.cbrt(0.001)");
add("Math.cbrt(2)");
add("Math.cbrt(Number.MIN_VALUE)");
add("Math.sign.length + ',' + Math.hypot.length + ',' + Math.max.length + ',' + Math.imul.length + ',' + Math.round.name");
add("Object.prototype.toString.call(Math)");

// ---- toLocaleString('en-US') básico.
for (const v of ["0", "-0", "1", "-1", "1234", "1234.5", "1234567.891", "-1234567.891", "0.1", "0.000001", "1e21", "1e-7", "123456789012345680000",
  "NaN", "Infinity", "-Infinity", "0.5", "999.9995", "1.005", "12345.6789", "1e6", "100", "1000", "0.9999999", "5e-324", "1.7976931348623157e308",
  "2**53", "-(2**31)", "1/3", "2/3", "99999.99999"]) {
  add(`(${v}).toLocaleString('en-US')`);
  add(`(${v}).toLocaleString()`);
  add(`(${v}).toLocaleString('en-US', { maximumFractionDigits: 0 })`);
  add(`(${v}).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })`);
  add(`(${v}).toLocaleString('en-US', { useGrouping: false })`);
  add(`(${v}).toLocaleString('en-US', { style: 'percent' })`);
  add(`(${v}).toLocaleString('en-US', { maximumSignificantDigits: 3 })`);
  add(`(${v}).toLocaleString('en-US', { style: 'currency', currency: 'USD' })`);
  add(`(${v}).toLocaleString('en-US', { notation: 'compact' })`);
}

// ---- Execução.
const vm = require("vm");
const { emitRow } = require("./golden-prelude.js");
const indirectEval = eval;
let kept = 0;
for (const body of programs) {
  const wrapped = `globalThis.R = (() => { const show = v => Object.is(v, -0) ? "-0" : typeof v === "bigint" ? v + "n" : typeof v === "string" ? v : typeof v === "object" && v !== null ? JSON.stringify(v) : String(v); try { return show(\n${body}\n); } catch (e) { return e.name + ": " + e.message; } })()`;
  // Programas de várias linhas (os do shortest repr) já gravam `R`; os de uma expressão passam pelo envoltório.
  const source = body.includes("globalThis.R") ? body : wrapped;
  let result;
  try {
    delete globalThis.R;
    indirectEval(source);
    result = globalThis.R === undefined ? "<undefined>" : String(globalThis.R);
  } catch (e) {
    result = e.name + ": " + e.message;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`programas ${kept}\n`);
