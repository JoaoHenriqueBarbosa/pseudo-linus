// Gera tests/golden/number_convert_bun.tsv: conversão exata entre string e número, medida no bun.
// Cobre Number()/parseFloat/unário + em strings de fronteira (meios-termos exatos entre doubles vizinhos, um dígito a
// mais ou a menos que o meio, 17+ dígitos, expoentes enormes, subnormais, Infinity com sinais e espaços Unicode,
// prefixos 0x/0b/0o em Number contra parseFloat, separadores _ que não valem em string), parseInt com radix 2..36,
// radix inválido e strings além de 2**53, Number.prototype.toString(radix) de frações em todos os radix, e
// toFixed/toExponential/toPrecision com todos os dígitos 0..100 e o RangeError exato fora da faixa.
// Programas cuja expressão já aparece nos goldens number_*, bigint_*, math, json_number e numeric_limits são descartados.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-numeric-limits-golden.js.
// Uso: bun scripts/gen-number-convert-golden.js > tests/golden/number_convert_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { spawn } = require("child_process");

const CHILD_LIMIT = 6;
const CHILD_TIMEOUT_MS = 8000;

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

// Literal de string JS só com ASCII: tudo acima de 0x7e sai como \uXXXX (evita travessão literal e separadores de linha).
const lit = text =>
  JSON.stringify(text).replace(/[\u007f-￿]/g, ch => "\\u" + ch.charCodeAt(0).toString(16).padStart(4, "0"));

const convert = text => {
  const l = lit(text);
  add(wrap(`Number(${l})`), wrap(`Number.parseFloat(${l})`), wrap(`+${l}`));
};

// ---- Aritmética exata sobre doubles.
const F = new Float64Array(1);
const U = new BigUint64Array(F.buffer);
const bitsOf = x => ((F[0] = x), U[0]);
const fromBits = b => ((U[0] = b), F[0]);
// x = m * 2**k, exato.
function decompose(x) {
  const bits = bitsOf(x);
  const e = (bits >> 52n) & 0x7ffn;
  const f = bits & ((1n << 52n) - 1n);
  return e === 0n ? { m: f, k: -1074 } : { m: f | (1n << 52n), k: Number(e) - 1075 };
}
// Decimal exato de m * 2**k como { digits, exp10 } (valor = digits * 10**exp10).
function exactDecimal(m, k) {
  if (k >= 0) return { digits: m << BigInt(k), exp10: 0 };
  return { digits: m * 5n ** BigInt(-k), exp10: k };
}
const sci = ({ digits, exp10 }) => `${digits}e${exp10}`;
// Decimal em notação posicional, sem expoente.
function plain({ digits, exp10 }) {
  const s = digits.toString();
  if (exp10 >= 0) return s + "0".repeat(exp10);
  const n = -exp10;
  if (s.length > n) return s.slice(0, s.length - n) + "." + s.slice(s.length - n);
  return "0." + "0".repeat(n - s.length) + s;
}

// ---- 1. String para número: meios-termos exatos e vizinhos.
let seed = 0x9e3779b97f4a7c15n;
const nextRandom = () => {
  seed = (seed * 6364136223846793005n + 1442695040888963407n) & ((1n << 64n) - 1n);
  return seed;
};
const specials = [
  1, 0.1, 0.2, 0.3, 4.35, 1e23, 1e22, 9007199254740991, 9007199254740992, 9007199254740994, 2 ** 53 + 4, 2 ** 54, 2 ** 63, 2 ** 64, 1e21, 123456789012345680000,
  5e-324, 1e-323, 2 ** -1022, 2.225073858507201e-308, 2.2250738585072009e-308, 1.7976931348623157e308, 8.98846567431158e307, 1e-7, 2 ** -1074 * 3, 2 ** -1073,
  3.141592653589793, 2.718281828459045, 0.5, 1.5, 4503599627370497, 4503599627370495.5, 1.0000000000000002, 0.9999999999999999, 5e-310, 2.5e-320,
];
const randoms = [];
for (let i = 0; i < 40; i++) {
  const r = nextRandom();
  const exponent = i < 8 ? (r >> 52n) % 4n : 1n + ((r >> 8n) % 2046n); // os primeiros são subnormais ou quase
  randoms.push(fromBits(((r >> 63n) & 0n) << 63n | (exponent << 52n) | (r & ((1n << 52n) - 1n))));
}
const halfwaySeen = new Set();
for (const x of [...specials, ...randoms]) {
  const { m, k } = decompose(x);
  const midpoint = exactDecimal(2n * m + 1n, k - 1);
  const forms = [
    sci(midpoint),
    sci({ digits: midpoint.digits * 10n + 1n, exp10: midpoint.exp10 - 1 }),
    sci({ digits: midpoint.digits * 10n - 1n, exp10: midpoint.exp10 - 1 }),
  ];
  if (midpoint.exp10 >= -80) forms.push(plain(midpoint));
  if (midpoint.exp10 >= -80) forms.push(plain({ digits: midpoint.digits * 10n + 1n, exp10: midpoint.exp10 - 1 }));
  for (const text of forms) {
    if (halfwaySeen.has(text)) continue;
    halfwaySeen.add(text);
    convert(text);
    add(wrap(`B(Number(${lit(text)}))`));
  }
}
// Dígitos significativos truncados do valor exato: 15 a 20, 25 e 40 dígitos.
for (const x of [0.1, 1 / 3, 2 / 3, Math.PI, 1e23, 5e-324, 2 ** -1022, 123.456, 0.3, 1.7976931348623157e308, 4.35, 9007199254740993 - 1, 2 ** 100, 1e-7]) {
  const { m, k } = decompose(x);
  const { digits, exp10 } = exactDecimal(m, k);
  const full = digits.toString();
  for (const count of [15, 16, 17, 18, 19, 20, 25, 40]) {
    if (count >= full.length) continue;
    const cut = full.slice(0, count);
    const scale = exp10 + full.length - count;
    convert(`${cut}e${scale}`);
    convert(`${cut[0]}.${cut.slice(1)}e${scale + count - 1}`);
    convert(`${cut}${"0".repeat(5)}1e${scale - 6}`);
  }
}

// ---- 2. String para número: casos escritos à mão.
const boundaryTexts = [
  "9007199254740993", "9007199254740995", "9007199254740993.0000000000000001", "9007199254740992.9999999999999999", "18014398509481985", "18014398509481987",
  "0.1000000000000000055511151231257827021181583404541015625", "0.10000000000000000555111512312578270211815834045410156250000001",
  "0.1000000000000000055511151231257827021181583404541015624999999", "1.00000000000000011102230246251565404236316680908203125",
  "1.00000000000000011102230246251565404236316680908203126", "1.00000000000000011102230246251565404236316680908203124",
  "2.2250738585072011e-308", "2.2250738585072012e-308", "2.2250738585072014e-308", "2.225073858507201136057409796709131975934819546351645648023426109724822222021076945516529523908135087914149158913039621106870086438694594645527657207407820621743379988141063267329253552286881372149012981122451451889849057222307285255133155755015914397476397983411801999323962548289017107081850690630666655994938275772572015763062690663332647565300009245888316433037779791869612049497390377829704905051080609940730262937128958950003583799967207254304360284078895771796150945516748243471030702609144621572289880258182545180325707018860872113128079512233426288368622321503775666622503982534335974568884423900265498198385487948292206894721689831099698365846814022854243330660339850886445804001034933970427567186443383770486037861622771738545623065874679014086723327636718751",
  "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "2.4703282292062327208e-324", "2.4703282292062327209e-324", "2.47032822920623272088284396434110686182529901307162382212792841250337753635104375932649918180817996189898282347722858865463328355177969898199387398005390939063150356595155702263922908583924491051844359318028499365361525003193704576782492193656236698636584807587739e-324",
  "1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308", "1.797693134862315807e308", "1.7976931348623158079372897140530341507993413271003782693617377898044496829276475094664901797758720709633028810720586334258624201236858124583218e308",
  "1.7976931348623158079372897140530341507993413271003782693617377898044496829276475094664901797758720709633028810720586334258624201236858124583217e308",
  "1.79769313486231580793728971405303415079934132710037826936173778980444968292764750946649017977587207096330286416692887910946555547851940402630657488671505820681908902000708383676273854845817711531764475730270069855571366959622842914819860834936475292719074168444365510704342711559699508093042880177904174497791e308",
  "1e308", "1e309", "-1e309", "1e-323", "1e-324", "-1e-324", "1e400", "-1e400", "1e-400", "1e99999", "1e-99999", "1e9999999999", "1e-9999999999", "1e99999999999999999999",
  "1e-99999999999999999999", "0e99999", "0e-99999", "0e99999999999999999999", "-0e1", "0.0e5", "0".repeat(400) + "1e400", "0.".padEnd(402, "0") + "1e400", "0.".padEnd(402, "0") + "1e401",
  "1".repeat(400), "1".repeat(309), "1".repeat(310), "9".repeat(309), "9".repeat(400), "0." + "9".repeat(400), "0." + "0".repeat(400) + "1", "0." + "0".repeat(323) + "5", "0." + "0".repeat(323) + "4",
  "0." + "0".repeat(323) + "2470328229206232720882843964341106861825299013071623822127928412503377536351043759326499181808179961898982823477228588654633283551779698981993873980053909390631503565951557022639229085839244910518443593180284993653615250031937045767824921936562366986365848075700158576926990370631192827955855133292783433840935197801553124659726357957462276646527282722005637400648549997709659947045402082816622623785739345073633900796776193057750674017632467360096895134053553745851525926551130085025036161325890626400116040009405468078180113124936166433251365099601253049573891168161497906764655097965399810606262973988166927990803776094246810868903625603738527",
  "1" + "0".repeat(308), "1" + "0".repeat(309), "1." + "0".repeat(400) + "1", "123456789012345678901234567890", "0.123456789012345678901234567890", "1234567890123456789012345678901234567890e-30",
  "0.00000000000000000000000000001e30", "1e0", "1E0", "1e+0", "1E+00", "1e-0", "1e", "1e+", "1e-", "e5", ".e5", ".5e", "5.e1", ".5e1", "+.5e+1", "-.5E-1", "0.e1", "00.5", "-00.5", "+00",
  "0x", "0X", "-0x1", "+0x1", "0x-1", "0x+1", "0x1.8", "0x1p3", "0x1e3", "0xg", "0x1g", "0xG", "0X1F", "0xFF", "0xff", "0xFfFf", "0x0", "0x00", "0x000000000000000000001", "0x20000000000000", "0x20000000000001",
  "0x20000000000002", "0x20000000000003", "0x20000000000005", "0x1fffffffffffff", "0x1fffffffffffff8", "0x1fffffffffffffc", "0x7fffffffffffffff", "0xffffffffffffffff", "0x" + "f".repeat(16),
  "0x" + "f".repeat(256), "0x" + "f".repeat(257), "0x" + "f".repeat(300), "0x" + "0".repeat(300) + "1", "0xfffffffffffff800" + "0".repeat(240), "0xfffffffffffffc00" + "0".repeat(240),
  "0b", "0B", "0b0", "0b1", "0B11", "0b102", "0b2", "-0b1", "+0b1", "0b" + "1".repeat(53), "0b" + "1".repeat(54), "0b1" + "0".repeat(53) + "1", "0b1" + "0".repeat(53) + "11", "0b" + "1".repeat(64),
  "0b" + "1".repeat(1024), "0b" + "1".repeat(1025), "0b1.1", "0b1e1", "0b10_1",
  "0o", "0O", "0o0", "0o7", "0o8", "0o17", "0O17", "-0o17", "+0o17", "0o" + "7".repeat(18), "0o" + "7".repeat(19), "0o" + "7".repeat(400), "0o1.5", "0o1e1", "0o1_7",
  "00", "01", "017", "08", "09.5", "0777", "-017", "0.5e1_0", "1_000", "1__000", "_1", "1_", "1_.5", "1._5", "1.5_5", "1e1_0", "1e_10", "0x1_0", "0b1_0", "0o1_0", "1_0e1", "1_0.0_1",
  "١٢٣", "１２３", "1٫5", "1,5", "1 000", "1 000", "1.5.5", "1..5", "..5", "1e5e5", "++1", "--1", "+-1", "-+1", "+ 1", "- 1", " +1", "+ 1", "1n", "1N", "0n", "NaN", "-NaN", "+NaN", "nan",
  "Infinity", "-Infinity", "+Infinity", "infinity", "INFINITY", "Infinit", "Infinityy", "Infinity1", "Infinity.", "Infinity e1", "Inf", "-Inf", "1/0", "0/0", "∞", "-∞", "undefined", "null", "true", "false", "[]", "{}", "[5]",
  "", " ", "\n", "\t\n\v\f\r  ﻿", "\u0000", "1\u0000", "\u00001", "1\u0000e1", "1e1\u0000", "᠎1", "1᠎", "​1", "1​", "\u0085 1", "1 \u0085", "  1  ",
  "0.1e-1", "0.1e1", "0.00000000000000001", "100000000000000000000", "1000000000000000000000", "123456789012345680000", "123456789012345680001", "0.000001", "0.0000001", "5e-324", "4e-324", "3e-324", "2.5e-324", "2.4e-324",
  "1.7976931348623157e+308", "1.7976931348623157E+308", "1.797693134862315708145274237317043567981e+308", "1.797693134862315807e+308",
];
for (const text of boundaryTexts) convert(text);

// Infinity com sinais e espaços Unicode (os brancos de StrWhiteSpaceChar, mais os que não valem).
const spaces = [
  " ", "\t", "\n", "\v", "\f", "\r", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ",
  " ", " ", " ", " ", "　", "﻿", "᠎", "​", "‌", "‍", "⁠", "\u0085", "\u001c", "\u001f", "\u0000", "­", "⠀",
];
for (const space of spaces) {
  for (const text of [
    space + "Infinity", "Infinity" + space, space + "-Infinity" + space, space + "+Infinity" + space, "-" + space + "Infinity", "+" + space + "Infinity", "Infinity" + space + "x",
    space, space + "1" + space, space + "0x10" + space, space + ".5e1" + space, space + "1" + space + "1", space + "NaN" + space, "1e" + space + "1",
  ]) convert(text);
}

// Contraste entre o literal numérico e a string, e Number com objetos.
add(wrap("1_000"), wrap("0x1_0"), wrap("0b1_1"), wrap("0o1_7"), wrap("1_0.0_1"), wrap("1e1_0"), wrap("0.0_1"), wrap("Number('1_000')"), wrap("parseFloat('1_000')"), wrap("parseInt('1_000')"),
  wrap("Number('0x1_0')"), wrap("Number('0b1_1')"), wrap("Number.parseFloat('0x1F')"), wrap("Number.parseFloat('0b11')"), wrap("Number.parseFloat('0o7')"),
  wrap("[Number('0x1F'),Number('0b11'),Number('0o17'),parseFloat('0x1F'),parseFloat('0b11'),parseFloat('0o17')].join()"),
  wrap("Number(new String('12'))"), wrap("Number({toString(){return '0x10'}})"), wrap("Number({valueOf(){return '1e3'}})"), wrap("Number(['5'])"), wrap("Number(['5','6'])"),
  wrap("Number([' 5 '])"), wrap("Number(Symbol())"), wrap("Number(1n)"), wrap("+1n"), wrap("Number(new Date(5))"), wrap("Number(null)"), wrap("Number(undefined)"), wrap("Number(true)"),
  wrap("Number()"), wrap("Number('')"), wrap("Number.parseFloat('')"), wrap("parseFloat('  -.5e-2xyz')"), wrap("parseFloat('1e1000')"), wrap("parseFloat('-1e1000')"),
  wrap("parseFloat('.1e1')"), wrap("parseFloat('1e+')"), wrap("parseFloat('-Infinityx')"), wrap("parseFloat('+Infinity1')"), wrap("parseFloat('-0')"), wrap("1/parseFloat('-0')"),
  wrap("1/Number('-0')"), wrap("1/+'-0.0e5'"), wrap("1/Number('-0x0')"), wrap("Object.is(Number('-0'),-0)"));

// ---- 3. parseInt com radix 2..36, radix inválido e strings longas.
const digitChar = v => v.toString(36);
for (let radix = 2; radix <= 36; radix++) {
  const top = digitChar(radix - 1);
  const topUpper = top.toUpperCase();
  const bad = digitChar(radix % 36 === 0 ? 35 : radix); // primeiro dígito que não vale no radix (se houver)
  const texts = [
    "10", "1", "0", "-10", "+10", "  10  ", top, top.repeat(5), top.repeat(12), top.repeat(20), top.repeat(54), top.repeat(100), topUpper.repeat(20),
    "1" + "0".repeat(60), "1" + "0".repeat(300), "1" + "0".repeat(1100), "0" + top.repeat(30), "9".repeat(25), "123456789012345678901234567890", "9007199254740993", "9007199254740992",
    "zz", "ZZ", "Zz9", "1" + top + "1" + bad + "1", bad, "0" + bad, "1" + bad, "10" + bad + "10", "1.9", "1e3", "0x1f", "0X1F", "-0x1f", "0b11", "0o17", "1_0", "10n", "١٠", "1 ", " 10", "1 0", "",
  ];
  for (const text of texts) add(wrap(`parseInt(${lit(text)},${radix})`));
  add(wrap(`parseInt(${lit("1" + "0".repeat(53))},${radix})`), wrap(`parseInt(${lit(top.repeat(11))},${radix})`), wrap(`parseInt(${lit(top.repeat(13))},${radix})`),
    wrap(`Number.parseInt(${lit(top.repeat(33))},${radix})`), wrap(`B(parseInt(${lit(top.repeat(40))},${radix}))`), wrap(`B(parseInt(${lit("1" + "0".repeat(60) + "1")},${radix}))`),
    wrap(`parseInt(${lit("-" + top.repeat(15))},${radix})`), wrap(`1/parseInt("-0",${radix})`), wrap(`parseInt("0x10",${radix})`), wrap(`parseInt("-0x10",${radix})`),
    wrap(`parseInt("0X10",${radix})`));
}
// Meios-termos de parseInt em potência de dois e decimal (arredondamento acima de 2**53).
for (const text of ["9007199254740993", "9007199254740995", "9007199254740993000", "18014398509481985", "18014398509481987", "36028797018963969", "36028797018963971"]) {
  add(wrap(`parseInt(${lit(text)})`), wrap(`parseInt(${lit(text)},10)`), wrap(`B(parseInt(${lit(text)}))`), wrap(`parseInt(${lit(text)}) === ${text.replace(/^(\d{16}).*/, "$1")}`));
}
for (const text of ["1" + "0".repeat(52) + "1", "1" + "0".repeat(53) + "1", "1" + "0".repeat(53) + "11", "1" + "0".repeat(54) + "1", "1".repeat(53), "1".repeat(54), "1".repeat(55), "1".repeat(64), "1".repeat(1024), "1".repeat(1025)]) {
  add(wrap(`parseInt(${lit(text)},2)`), wrap(`B(parseInt(${lit(text)},2))`));
}
for (const text of ["20000000000001", "20000000000003", "20000000000005", "1fffffffffffff", "1fffffffffffff8", "ffffffffffffffff", "f".repeat(256), "f".repeat(257), "f".repeat(300), "fffffffffffff800" + "0".repeat(240)]) {
  add(wrap(`parseInt(${lit(text)},16)`), wrap(`B(parseInt(${lit(text)},16))`), wrap(`parseInt(${lit("0x" + text)})`), wrap(`parseInt(${lit("0x" + text)},16)`));
}
const radixValues = [
  "0", "1", "2", "10", "16", "36", "37", "38", "-1", "-2", "-0", "+0", "NaN", "Infinity", "-Infinity", "undefined", "null", "true", "false", "''", "' '", "'16'", "'0x10'", "'z'", "[]", "[16]", "[2,3]", "{}",
  "{valueOf(){return 8}}", "{toString(){return '2'}}", "16.1", "16.9", "-16.9", "2.9", "36.9", "37.1", "0.5", "-0.5", "1e3", "1e10", "4294967296", "4294967297", "4294967312", "4294967306", "4294967330", "4294967332",
  "-4294967280", "2147483648", "2147483664", "-2147483648", "-2147483632", "9007199254740992", "2**53+16", "1n", "Symbol()", "new Number(16)", "new String('8')",
];
for (const radix of radixValues) {
  for (const text of ["10", "z", "11", "0x10", "-0x10", "77", "Zz", "", "12345678901234567890", "1".repeat(60)]) add(wrap(`parseInt(${lit(text)},${radix})`));
}
add(wrap("parseInt()"), wrap("parseInt(undefined)"), wrap("parseInt(undefined,36)"), wrap("parseInt(null)"), wrap("parseInt(null,36)"), wrap("parseInt(NaN,36)"), wrap("parseInt(Infinity,36)"),
  wrap("parseInt(-Infinity,36)"), wrap("parseInt(true,36)"), wrap("parseInt(false,36)"), wrap("parseInt({},36)"), wrap("parseInt([],36)"), wrap("parseInt([10,20])"), wrap("parseInt(1e21)"), wrap("parseInt(1e20)"),
  wrap("parseInt(123456789012345680000)"), wrap("parseInt(0.0000001)"), wrap("parseInt(0.00000001)"), wrap("parseInt(5e-324)"), wrap("parseInt(-5e-324)"), wrap("1/parseInt(-5e-324)"), wrap("1/parseInt(-0.9)"),
  wrap("parseInt(1e21,36)"), wrap("parseInt(0.0000005,36)"), wrap("parseInt(2**53)"), wrap("parseInt(2**53+2)"), wrap("parseInt(-(2**70))"), wrap("parseInt(1.7976931348623157e308)"),
  wrap("parseInt(Number.MAX_SAFE_INTEGER+2)"), wrap("parseInt(Symbol())"), wrap("parseInt('1',Symbol())"), wrap("parseInt(1n)"), wrap("parseInt('1',1n)"), wrap("parseInt({toString(){throw 7}})"),
  wrap("parseInt('1',{valueOf(){throw 8}})"), wrap("parseInt.length"), wrap("parseInt.name"), wrap("Number.parseInt===parseInt"), wrap("parseFloat.length"), wrap("new parseInt('1')"));

// ---- 4. Number.prototype.toString(radix) de frações em todos os radix.
const fractionValues = [
  "0.1", "0.2", "0.3", "0.7", "0.9", "0.99", "0.999999", "1/7", "2/3", "1/9", "1/11", "1/13", "1.1", "2.2", "10.1", "100.01", "12345.6789", "3.14159", "-2.71828", "-0.1", "-1/3", "0.30000000000000004",
  "2**-1", "2**-2", "2**-10", "2**-30", "2**-52", "2**-53", "2**-60", "2**-100", "2**-1022", "2**-1074", "1+2**-52", "1-2**-53", "1e-5", "1e-6", "1e-7", "1e-10", "1e-15", "1e-20", "1e-100", "1e-300",
  "123456789.123456789", "1e15+0.5", "2**52+0.5", "2**53-1", "2**53", "2**53+2", "2**60", "2**64", "2**70+2**20", "1e21", "1e22", "1e23", "1e25", "1e50", "1e100", "1e200", "1.7976931348623157e308",
  "Number.EPSILON", "Number.MAX_SAFE_INTEGER", "-Number.MAX_SAFE_INTEGER", "Math.SQRT2", "Math.LN2", "Math.LN10", "Math.LOG2E", "Math.SQRT1_2", "4.35", "0.5+2**-30", "255.255", "0.0001",
];
for (const value of fractionValues) {
  for (let radix = 2; radix <= 36; radix++) add(wrap(`(${value}).toString(${radix})`));
}
// Casos de fração gerados com valores pseudoaleatórios em [0, 1) e em escalas variadas.
for (let i = 0; i < 40; i++) {
  const r = nextRandom();
  const mantissa = Number(r >> 11n) / 2 ** 53;
  const scale = [1, 1, 10, 1000, 1e6, 1e-3, 1e-8, 2 ** 40, 1][i % 9];
  const value = mantissa * scale;
  for (const radix of [2, 3, 5, 7, 8, 11, 13, 16, 17, 20, 25, 30, 32, 35, 36]) add(wrap(`(${value}).toString(${radix})`));
}

// ---- 5. toFixed / toExponential / toPrecision com todos os dígitos.
const digitValues = [
  "0.5", "1.005", "123.456", "1e-7", "0.000001234", "2**-30", "1e20", "123456789012345680000", "1.7976931348623157e308", "5e-324", "-0.1", "1/3", "2/3", "0.1+0.2", "Math.PI", "9.995", "99.5", "1e21-65536",
];
for (const value of digitValues) {
  for (let digits = 0; digits <= 100; digits++) {
    add(wrap(`(${value}).toFixed(${digits})`), wrap(`(${value}).toExponential(${digits})`));
    if (digits >= 1) add(wrap(`(${value}).toPrecision(${digits})`));
  }
}
const smallValues = ["0", "-0", "1", "-1", "10", "99", "999", "1000", "0.9", "0.99", "0.999", "9.5", "9.95", "9.995", "0.05", "0.005", "0.0005", "5", "15", "25", "35", "1.25", "1.35", "2.675", "1.45"];
for (const value of smallValues) {
  for (let digits = 0; digits <= 22; digits++) {
    add(wrap(`(${value}).toFixed(${digits})`), wrap(`(${value}).toExponential(${digits})`));
    if (digits >= 1) add(wrap(`(${value}).toPrecision(${digits})`));
  }
}
// Dígitos fora da faixa: o RangeError e a ordem entre a checagem de dígitos e o valor especial.
const badDigits = ["-1", "-0.5", "-2", "101", "102", "1000", "2**31", "2**32", "2**32+1", "Infinity", "-Infinity", "NaN", "'abc'", "{}", "[]", "[5]", "'5'", "5.9", "null", "true", "undefined", "1n", "Symbol()", "{valueOf(){return 101}}", "{valueOf(){throw 3}}"];
const badReceivers = ["1.5", "0", "-0", "NaN", "Infinity", "-Infinity", "1e21", "-1e21", "123.456", "1e-7"];
for (const receiver of badReceivers) {
  for (const digits of badDigits) {
    add(wrap(`(${receiver}).toFixed(${digits})`), wrap(`(${receiver}).toExponential(${digits})`), wrap(`(${receiver}).toPrecision(${digits})`));
  }
}
for (const receiver of ["NaN", "Infinity", "-Infinity", "1e21", "-1e21", "1e300", "0", "-0"]) {
  for (const digits of ["0", "1", "2", "20", "100"]) add(wrap(`(${receiver}).toFixed(${digits})`), wrap(`(${receiver}).toExponential(${digits})`), wrap(`(${receiver}).toPrecision(${digits || 1})`));
}
add(wrap("(1.5).toPrecision(0)"), wrap("(NaN).toPrecision(0)"), wrap("(Infinity).toPrecision(0)"), wrap("(1.5).toPrecision(undefined)"), wrap("(1.5).toPrecision()"), wrap("(1.5).toExponential()"),
  wrap("(1.5).toExponential(undefined)"), wrap("(0).toExponential()"), wrap("(123456).toExponential()"), wrap("(1e-7).toExponential()"), wrap("(5e-324).toExponential()"), wrap("(1.7976931348623157e308).toExponential()"),
  wrap("(1.5).toFixed()"), wrap("(1.5).toFixed(undefined)"), wrap("(1e21).toFixed()"), wrap("(0.5).toFixed()"), wrap("(2.5).toFixed()"), wrap("(-2.5).toFixed()"), wrap("(-0.5).toFixed()"), wrap("(-0.0001).toFixed(2)"),
  wrap("(1.45).toFixed(1)"), wrap("(8.345).toFixed(2)"), wrap("(1.005).toFixed(2)"), wrap("(10.235).toFixed(2)"), wrap("(0.000001).toFixed(7)"), wrap("(1e-10).toFixed(100)"), wrap("(123.456).toFixed(100)"),
  wrap("Number.prototype.toFixed.call('1',2)"), wrap("Number.prototype.toExponential.call({},2)"), wrap("Number.prototype.toPrecision.call(null,2)"), wrap("Number.prototype.toFixed.call(new Number(1.5),1)"),
  wrap("Number.prototype.toFixed.length"), wrap("Number.prototype.toExponential.length"), wrap("Number.prototype.toPrecision.length"), wrap("Number.prototype.toString.length"),
  wrap("(5).toString(1)"), wrap("(5).toString(37)"), wrap("(5).toString(0)"), wrap("(5).toString(-1)"), wrap("(5).toString(Infinity)"), wrap("(NaN).toString(1)"), wrap("(NaN).toString(37)"),
  wrap("(Infinity).toString(1)"), wrap("(0).toString(37)"), wrap("(5).toString(undefined)"), wrap("(5).toString(null)"), wrap("(5).toString({valueOf(){return 37}})"), wrap("(5).toString(2.9)"),
  wrap("(5).toString('2')"), wrap("(5).toString(Symbol())"), wrap("(5).toString(1n)"), wrap("(5).toString(4294967298)"), wrap("(5).toString(4294967330)"), wrap("(-0).toString(2)"), wrap("(-0).toString(16)"));

// ---- Execução.
const baseSources = knownPrograms("number_convert_bun.tsv", file => /^(number_|bigint|math|json_number|numeric_limits)/.test(file) && file.endsWith("_bun.tsv") && !file.startsWith("number_convert"));
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
// No máximo CHILD_LIMIT filhos por vez, cada um com timeout; a saída sai na ordem dos programas.
const runChild = source =>
  new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), CHILD_TIMEOUT_MS);
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => {
      clearTimeout(timer);
      const decoded = code === 0 ? decodeResult(out) : null;
      decoded !== null ? resolve(decoded) : reject(new Error(err || "filho falhou ou estourou o tempo"));
    });
    child.stdin.end(source);
  });
(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: CHILD_LIMIT }, async () => {
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
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || /[–—]/.test(result + source)) {
      dropped++;
      process.stderr.write("caminho, marca ou travessão no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    rows.push({ source, result });
  });
  fs.writeSync(1, emitFactored("number_convert", rows));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
})();
