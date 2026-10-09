// Gera tests/golden/numeric_limits_bun.tsv: limites de Number, BigInt e Math, medido no bun 1.4.2.
// Cobre Number.prototype.toString(radix) com fracionários em radix 2..36, toFixed/toPrecision/toExponential nos limites
// (1e21, 1e-7, 0.5, -0), Number.parseFloat/parseInt com lixo, hex, octal e separadores, BigInt.asIntN/asUintN com bits
// grandes, BigInt toString(radix), BigInt de strings com espaços e prefixos, aritmética BigInt (**, %, >>, divisão por
// zero, mistura com Number) e Math.round/fround/clz32/hypot/expm1/log1p/cbrt/sumPrecise nas bordas.
// Programas cuja expressão já aparece nos goldens number_*, bigint_*, math e json_number são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-numeric-limits-golden.js > tests/golden/numeric_limits_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  "var F64=new Float64Array(1),U64=new BigUint64Array(F64.buffer);\n" +
  "function B(v){F64[0]=v;return U64[0].toString(16)}\n";

const exprs = [];
const add = (...list) => exprs.push(...list);
const wrap = body => `T(()=>${body})`;

// ---- 1. Number.prototype.toString(radix) com fracionários.
const fractions = [
  "0.5", "0.1", "1/3", "-0.75", "255.5", "123.456", "2**-20", "0.000001", "1e21", "1e-7", "Number.MAX_VALUE", "Number.MIN_VALUE",
  "Number.EPSILON", "-1e300", "0.1+0.2", "Math.PI", "Math.E", "1e-10", "4294967296.5", "-0",
];
for (const value of fractions) {
  for (let radix = 2; radix <= 36; radix++) add(wrap(`(${value}).toString(${radix})`));
}
for (const radix of ["0", "1", "37", "-1", "'16'", "undefined", "null", "NaN", "2.9", "36.5", "{valueOf(){return 8}}", "1e3", "Infinity"]) {
  for (const value of ["255", "0.5", "NaN", "-Infinity", "-0"]) add(wrap(`(${value}).toString(${radix})`));
}

// ---- 2. toFixed / toPrecision / toExponential nos limites.
const formatValues = [
  "1e21", "-1e21", "1e20", "1e-7", "1e-6", "0.5", "1.5", "2.5", "-0", "0", "-1.5", "1.005", "123.456", "0.000001", "0.1+0.2", "NaN",
  "Infinity", "-Infinity", "9.995", "99.5", "0.045", "1.45", "8.345", "1.255", "5e-324", "1.7976931348623157e308", "123456789012345680000",
  "0.00001", "25", "-0.5", "1e21-1", "0.3", "2**53", "-(2**53)",
];
for (const value of formatValues) {
  for (const digits of ["0", "1", "2", "3", "5", "10", "20", "50", "100", "-1", "101", "undefined", "'2'", "NaN", "1.9"]) {
    add(wrap(`(${value}).toFixed(${digits})`), wrap(`(${value}).toExponential(${digits})`));
  }
  for (const digits of ["1", "2", "3", "5", "10", "21", "50", "100", "0", "101", "undefined", "'4'", "NaN", "2.9"]) {
    add(wrap(`(${value}).toPrecision(${digits})`));
  }
}
add(wrap("Number.prototype.toFixed.call('1')"), wrap("Number.prototype.toPrecision.call({})"), wrap("Number.prototype.toExponential.call(null)"),
  wrap("Number.prototype.toString.call(Object(5),2)"), wrap("new Number(0.5).toString(2)"));

// ---- 3. parseFloat / parseInt / Number com lixo, hex, octal e separadores.
const texts = [
  "", " ", "  12  ", "12abc", "abc12", "0x1F", "0X1f", "0b101", "0o17", "017", "08", "1_000", "1,000", "1e3", "1E-3", "1e", "1e+", ".5", "5.", ".", "-.5",
  "+.5e1", "--1", "+-1", "Infinity", "-Infinity", "+Infinity", "infinity", "Infinityx", "NaN", "0x", "0x.8", "1.2.3", "1e1000", "-1e1000", "1e-1000",
  " 12", " 1 ", "﻿7", "​5", "1 ", "9007199254740993", "0.1e1", "00012", "-0", "+0", "0e0", "0.0000001", "123456789012345678901234567890",
  "0x1p3", "1n", "١٢", "12\u0000", "\n\t 3.5e2xyz", "0.5.5", "1e5.5", "- 1", "Infinit", "1_0",
];
for (const text of texts) {
  const literal = JSON.stringify(text);
  add(wrap(`Number.parseFloat(${literal})`), wrap(`Number.parseInt(${literal})`), wrap(`Number(${literal})`), wrap(`+${literal}`));
  for (const radix of ["2", "8", "10", "16", "36", "0", "1", "37", "undefined", "-1", "4294967312", "16.9"]) add(wrap(`Number.parseInt(${literal},${radix})`));
}
add(wrap("Number.parseFloat===parseFloat"), wrap("Number.parseInt===parseInt"), wrap("parseInt(null,36)"), wrap("parseInt('Infinity',36)"),
  wrap("parseInt(0.0000005)"), wrap("parseInt(1e21)"), wrap("parseInt(-0)"), wrap("1/parseInt('-0')"), wrap("parseInt('z',36)"), wrap("parseInt('zz',35)"),
  wrap("parseFloat({toString(){return '3.5x'}})"), wrap("parseInt({valueOf(){return 7}})"), wrap("parseFloat(Symbol())"), wrap("parseInt(1n)"));

// ---- 4. BigInt.asIntN / asUintN com bits grandes.
const bigValues = [
  "0n", "1n", "-1n", "127n", "128n", "255n", "256n", "-128n", "-129n", "2n**63n", "2n**63n-1n", "-(2n**63n)", "2n**64n", "2n**64n-1n", "-(2n**64n)-1n",
  "2n**128n+5n", "-(2n**128n)", "2n**200n-1n", "-(2n**1000n)+7n", "10n**30n", "-(10n**30n)",
];
for (const value of bigValues) {
  for (const bits of ["0", "1", "2", "7", "8", "31", "32", "53", "63", "64", "65", "127", "128", "129", "200", "1000", "4096", "2**32", "2**53-1", "2**53", "-1", "1.5", "'8'", "undefined", "NaN", "{valueOf(){return 4}}"]) {
    add(wrap(`BigInt.asIntN(${bits},${value})`), wrap(`BigInt.asUintN(${bits},${value})`));
  }
}
add(wrap("BigInt.asIntN(8,1)"), wrap("BigInt.asUintN(8,'1')"), wrap("BigInt.asIntN(8)"), wrap("BigInt.asIntN()"), wrap("BigInt.asIntN(8,Object(5n))"),
  wrap("BigInt.asUintN(0,5n)===0n"), wrap("BigInt.asIntN.length+BigInt.asUintN.length"), wrap("new BigInt.asIntN(1,1n)"));

// ---- 5. BigInt toString(radix) e de strings.
const toStringValues = ["0n", "-0n", "1n", "-1n", "255n", "-255n", "2n**64n", "-(2n**64n)", "10n**40n", "35n", "36n", "2n**100n-1n", "-(2n**128n)", "123456789012345678901234567890n"];
for (const value of toStringValues) {
  for (let radix = 2; radix <= 36; radix += value.length > 8 ? 1 : 3) add(wrap(`(${value}).toString(${radix})`));
  for (const radix of ["0", "1", "37", "undefined", "'16'", "2.5", "null", "{valueOf(){return 16}}"]) add(wrap(`(${value}).toString(${radix})`));
}
const bigTexts = [
  "", " ", "  12  ", "\n\t7\r", "0", "-0", "+5", "-5", "+-5", "--5", "0x1f", "0X1F", "-0x1f", "+0x1f", "0b101", "0B11", "0o17", "0O7", "-0b1", "0x", "0b", "0o", "0b2", "0o8", "0xg",
  "1_000", "1n", "1.0", "1.5", ".5", "5.", "1e3", "Infinity", "NaN", "00012", "017", "08", "12abc", " 5 ", "﻿9", " 1 ", "​5", "١٢",
  "123456789012345678901234567890", "-123456789012345678901234567890", "0x" + "f".repeat(40), "0b" + "1".repeat(70), "9".repeat(60), "0".repeat(30) + "1",
];
for (const text of bigTexts) {
  const literal = JSON.stringify(text);
  add(wrap(`BigInt(${literal})`), wrap(`BigInt.asUintN(64,BigInt(${literal}))`), wrap(`${literal}==5n`), wrap(`${literal}==0n`), wrap(`0n<${literal}`));
}
add(wrap("BigInt(1.5)"), wrap("BigInt(NaN)"), wrap("BigInt(Infinity)"), wrap("BigInt(-0)"), wrap("BigInt(1e21)"), wrap("BigInt(2**53)"), wrap("BigInt(Number.MAX_VALUE)"),
  wrap("BigInt(Number.MIN_VALUE)"), wrap("BigInt(true)"), wrap("BigInt(false)"), wrap("BigInt(null)"), wrap("BigInt(undefined)"), wrap("BigInt(Symbol())"),
  wrap("BigInt({})"), wrap("BigInt([])"), wrap("BigInt([7])"), wrap("BigInt({valueOf(){return 3}})"), wrap("BigInt({valueOf(){return 3.5}})"),
  wrap("new BigInt(1)"), wrap("BigInt()"), wrap("BigInt(Object(4n))"), wrap("BigInt('1'.repeat(400)).toString().length"), wrap("BigInt(-1e300).toString().length"));

// ---- 6. Aritmética BigInt.
const operands = ["0n", "1n", "-1n", "2n", "-2n", "3n", "-3n", "7n", "-7n", "10n", "2n**64n", "-(2n**64n)", "2n**64n+1n", "-(2n**100n)+3n", "12345678901234567890n"];
for (const a of operands) {
  for (const b of operands) {
    add(wrap(`${a}/${b}`), wrap(`${a}%${b}`), wrap(`${a}**${b}`));
  }
  for (const shift of ["0n", "1n", "5n", "63n", "64n", "65n", "200n", "-1n", "-5n", "-64n", "-200n", "2n**40n"]) {
    add(wrap(`${a}>>${shift}`), wrap(`${a}<<${shift}`));
  }
  for (const other of ["1", "0", "1.5", "NaN", "-0", "'1'", "true", "null", "undefined", "Symbol()", "1n"]) {
    add(wrap(`${a}+${other}`), wrap(`${a}*${other}`), wrap(`${a}>>>${other}`), wrap(`${a}<${other}`), wrap(`${a}==${other}`));
  }
  add(wrap(`-${a}`), wrap(`~${a}`), wrap(`+${a}`), wrap(`${a}>>>0n`), wrap(`typeof Object(${a})`), wrap(`Number(${a})`), wrap(`Math.abs(${a})`),
    wrap(`${a}&-1n`), wrap(`${a}|1n`), wrap(`${a}^-1n`), wrap(`${a}++`.replace(/^(.*)\+\+$/, "((x)=>++x)($1)")), wrap(`(${a}).toLocaleString()`), wrap(`parseInt(${a})`), wrap(`JSON.stringify(${a})`));
}
add(wrap("2n**-1n"), wrap("0n**0n"), wrap("(-2n)**3n"), wrap("(-2n)**64n"), wrap("2n**(2n**40n)"), wrap("1n**(2n**80n)"), wrap("(-1n)**(2n**80n+1n)"),
  wrap("0n**(2n**80n)"), wrap("(2n**1000n)**10n"), wrap("5n/0n"), wrap("5n%0n"), wrap("0n/0n"), wrap("-5n/0n"), wrap("5/0n"), wrap("5n/0"), wrap("5%0n"),
  wrap("(-5n)%3n"), wrap("5n%(-3n)"), wrap("(-5n)/3n"), wrap("5n/(-3n)"), wrap("(-(2n**64n))>>64n"), wrap("(-(2n**64n)-1n)>>64n"), wrap("-1n>>1000n"), wrap("1n>>1000n"),
  wrap("1n<<(2n**40n)"), wrap("0n<<(2n**40n)"), wrap("1n>>(2n**40n)"), wrap("-1n>>(2n**70n)"), wrap("1n<<-(2n**70n)"), wrap("1n<<2n**30n"),
  wrap("5n>>>1n"), wrap("5n>>>0"), wrap("1n+1"), wrap("1n*'2'"), wrap("1n+'2'"), wrap("'2'+1n"), wrap("`${1n}`"), wrap("1n+{}"), wrap("1n+[]"),
  wrap("1n+{valueOf(){return 1n}}"), wrap("1n+{valueOf(){return 1}}"), wrap("Math.max(1n)"), wrap("Math.max(1,2n)"), wrap("Number(2n**1024n)"),
  wrap("Number(2n**1024n-1n)"), wrap("Number(2n**1023n)"), wrap("Number(-(2n**1024n))"), wrap("Number(2n**53n+1n)"), wrap("Number(2n**53n+3n)"),
  wrap("Number((2n**54n)+2n)"), wrap("Number((2n**54n)+6n)"), wrap("Number(2n**64n-1n)"), wrap("Number(2n**65n+(2n**12n))"), wrap("Number(2n**65n+(2n**12n)+1n)"),
  wrap("2n**53n==2**53"), wrap("2n**53n+1n==2**53"), wrap("2n**53n+1n>2**53"), wrap("1n==1.5"), wrap("1n<1.5"), wrap("2n>1.5"), wrap("1n<Infinity"),
  wrap("1n>-Infinity"), wrap("1n<NaN"), wrap("1n>NaN"), wrap("1n==NaN"), wrap("0n==-0"), wrap("0n===-0"), wrap("1n<'2'"), wrap("1n<'x'"), wrap("2n**64n<'18446744073709551617'"),
  wrap("[3n,1n,2n,-1n,0n].sort().join()"), wrap("[3n,1n,2n,-1n,0n].sort((a,b)=>a<b?-1:a>b?1:0).join()"), wrap("BigInt.prototype.valueOf.call(1)"),
  wrap("BigInt.prototype.toString.call(1)"), wrap("BigInt.prototype.toLocaleString.call(5n)"), wrap("Object.prototype.toString.call(1n)"),
  wrap("BigInt.prototype[Symbol.toStringTag]"), wrap("typeof BigInt.prototype.valueOf.call(Object(3n))"));

// ---- 7. Math nas bordas.
const mathValues = [
  "0", "-0", "0.5", "-0.5", "1.5", "-1.5", "2.5", "-2.5", "0.49999999999999994", "-0.49999999999999994", "4503599627370495.5", "4503599627370496.5", "9007199254740991",
  "-9007199254740991", "2**52", "2**53", "1e21", "-1e21", "1e-7", "5e-324", "-5e-324", "Number.MAX_VALUE", "Infinity", "-Infinity", "NaN", "1", "-1", "2", "3", "10", "100",
  "1e-10", "1e10", "Math.PI", "-Math.PI", "0.1", "0.3", "27", "-27", "8", "1e300", "-1e300", "1e-300", "2**-1074", "2**-1022", "2**1023", "709", "710", "-745", "-746", "0.9999999999999999",
  "-1+2**-53", "1+2**-52", "4294967295", "4294967296", "4294967297", "-4294967297", "2147483648", "-2147483649", "65536", "16777216", "16777217", "3.4028235677973366e38", "3.4028234663852886e38",
  "1e-46", "1.401298464324817e-45", "7e-46", "0.1+0.2", "Number.MIN_VALUE*2",
];
for (const value of mathValues) {
  for (const fn of ["round", "fround", "clz32", "expm1", "log1p", "cbrt", "trunc", "sign", "fround"]) add(wrap(`Math.${fn}(${value})`), wrap(`B(Math.${fn}(${value}))`));
  add(wrap(`Math.hypot(${value})`), wrap(`Math.hypot(${value},${value})`), wrap(`Math.hypot(${value},3,4)`), wrap(`Math.hypot(${value},NaN)`), wrap(`Math.hypot(${value},Infinity)`),
    wrap(`Math.sumPrecise([${value}])`), wrap(`Math.sumPrecise([${value},${value}])`), wrap(`Math.sumPrecise([${value},-${value}])`), wrap(`Math.sumPrecise([1e308,${value},-1e308])`),
    wrap(`B(Math.hypot(${value},1))`), wrap(`B(Math.sumPrecise([${value},0.1,0.2]))`));
}
add(wrap("Math.hypot()"), wrap("Math.hypot(-0)"), wrap("Math.hypot(-0,-0)"), wrap("1/Math.hypot(-0)"), wrap("Math.hypot(1e200,1e200)"), wrap("Math.hypot(1e-200,1e-200)"),
  wrap("Math.hypot(3,4,12)"), wrap("Math.hypot('3','4')"), wrap("Math.hypot(3n)"), wrap("Math.hypot(Infinity,NaN)"), wrap("Math.hypot(NaN,Infinity)"), wrap("Math.hypot(1e308,1e308)"),
  wrap("Math.hypot({valueOf(){return 3}},4)"), wrap("Math.hypot.length"), wrap("Math.hypot(...Array(100).fill(1))"), wrap("Math.hypot(...Array(1000).fill(0.1))"),
  wrap("Math.sumPrecise([])"), wrap("1/Math.sumPrecise([])"), wrap("1/Math.sumPrecise([-0])"), wrap("1/Math.sumPrecise([-0,-0])"), wrap("1/Math.sumPrecise([-0,0])"),
  wrap("Math.sumPrecise([1e20,0.1,-1e20])"), wrap("Math.sumPrecise([0.1,0.2,0.3])"), wrap("Math.sumPrecise([1,1e100,1,-1e100])"), wrap("Math.sumPrecise([Infinity,-Infinity])"),
  wrap("Math.sumPrecise([Infinity,1])"), wrap("Math.sumPrecise([NaN,1])"), wrap("Math.sumPrecise([1,'2'])"), wrap("Math.sumPrecise([1n])"), wrap("Math.sumPrecise(1)"),
  wrap("Math.sumPrecise()"), wrap("Math.sumPrecise('12')"), wrap("Math.sumPrecise(new Set([1,2,3]))"), wrap("Math.sumPrecise({[Symbol.iterator]:function*(){yield 1;yield 2}})"),
  wrap("Math.sumPrecise([1.7976931348623157e308,1.7976931348623157e308])"), wrap("Math.sumPrecise([1.7976931348623157e308,1.7976931348623157e308,-1.7976931348623157e308])"),
  wrap("Math.sumPrecise([5e-324,5e-324])"), wrap("Math.sumPrecise([2**-1074,-(2**-1074)])"), wrap("Math.sumPrecise([2**53,1,1])"), wrap("Math.sumPrecise([2**53,1,1,1])"),
  wrap("Math.sumPrecise([-(2**53),-1,-1])"), wrap("Math.sumPrecise(Array(1000).fill(0.1))"), wrap("Math.sumPrecise(Array(10).fill(0.1))"), wrap("Math.sumPrecise.length"),
  wrap("Math.sumPrecise.name"), wrap("Math.sumPrecise([1,2,3]).toFixed(1)"),
  wrap("Math.clz32()"), wrap("Math.clz32('8')"), wrap("Math.clz32(-1)"), wrap("Math.clz32(0.9)"), wrap("Math.clz32(2**32)"), wrap("Math.clz32(2**32+1)"), wrap("Math.clz32(1n)"),
  wrap("Math.round()"), wrap("Math.round('2.5')"), wrap("Math.round(null)"), wrap("Math.round(true)"), wrap("Math.round([2.5])"), wrap("Math.round(1n)"), wrap("Math.round.length"),
  wrap("Math.fround()"), wrap("Math.fround('1.1')"), wrap("Math.fround(1n)"), wrap("Math.cbrt(Object(8))"), wrap("Math.expm1()"), wrap("Math.log1p()"), wrap("Math.log1p(-1)"),
  wrap("Math.log1p(-1.0000001)"), wrap("Math.log1p(-0.9999999999999999)"), wrap("Math.expm1(-Infinity)"), wrap("Math.expm1(710)"), wrap("Math.expm1(1e-300)"),
  wrap("Math.cbrt(-0)"), wrap("1/Math.cbrt(-0)"), wrap("Math.cbrt(1e-320)"), wrap("Math.cbrt(Number.MIN_VALUE)"), wrap("Math.cbrt(0.001)"), wrap("Math.cbrt(1000)"),
  wrap("Math.cbrt(3.375)"), wrap("Math.cbrt(-1e-10)"), wrap("Math.round(-0.5)"), wrap("1/Math.round(-0.5)"), wrap("1/Math.round(-0)"), wrap("1/Math.round(0.4)"),
  wrap("1/Math.round(-0.4)"), wrap("Math.round(2**52+0.5)"), wrap("Math.round(-(2**52)-0.5)"), wrap("Math.round(Number.MAX_VALUE)"));

// ---- Execução.
const baseSources = [];
baseSources.push(...knownPrograms("numeric_limits_bun.tsv", (file) => !(!/^(number_|bigint|math|json_number|numeric_limits)/.test(file) || !file.endsWith(".tsv")) && !(file.startsWith("numeric_limits"))));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
const jobs = [];
for (const expr of unique) {
  if (baseText.includes("globalThis.R = " + expr)) { dup++; continue; }
  jobs.push({ expr, source: '"use strict";\n' + PRELUDE + "globalThis.R = " + expr });
}
// Processo fresco por programa, igual aos demais goldens: o JSC reifica tabelas estáticas por ordem de acesso.
// Rodam em paralelo (8 por vez) e a saída sai na ordem dos programas.
const runChild = source =>
  new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => {
      const decoded = code === 0 ? decodeResult(out) : null;
      decoded !== null ? resolve(decoded) : reject(new Error(err || "filho falhou"));
    });
    child.stdin.end(source);
  });
(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: 8 }, async () => {
      while (next < jobs.length) {
        const index = next++;
        try { results[index] = await runChild(jobs[index].source); } catch (e) { results[index] = e; }
      }
    }),
  );
  const rows = [];
  jobs.forEach(({ expr, source }, index) => {
    const result = results[index];
    if (result instanceof Error) {
      process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + result + "\n");
      dropped++;
      return;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    rows.push({ source, result });
  });
  fs.writeSync(1, emitFactored("numeric_limits", rows));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
