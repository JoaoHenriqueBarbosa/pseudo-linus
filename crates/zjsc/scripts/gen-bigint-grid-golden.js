// Gera tests/golden/bigint_grid_bun.tsv: BigInt em grade, medido no bun 1.4.2.
// Cobre BigInt.asIntN/asUintN (bits de fronteira e inválidos, valores de fronteira positivos e negativos), os operadores
// com BigInt de 1 a 300 bits (+ - * / % ** << >> & | ^ ~ e - unário, divisão por zero, expoente negativo, >>> TypeError),
// BigInt(x) de strings (prefixos, espaços, sinais, inválidas) e de números não inteiros, toString(radix) e comparação de
// BigInt com Number e string (==, <, NaN, Infinity).
// Colunas: a fonte do programa (JSON), o valor da variável global `R` (JSON) e, se houver, o índice do prelúdio.
// Cada programa roda num bun filho novo (a tabela estática do JSC reifica por ordem de acesso), no máximo 6 ao mesmo
// tempo e com timeout de 8 s. Os programas já presentes em goldens vizinhos de BigInt são pulados.
// Uso: bun scripts/gen-bigint-grid-golden.js > tests/golden/bigint_grid_bun.tsv
const { emitFactoredLines, knownPrograms, sampleByHash } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'var S=v=>typeof v=="symbol"?String(v):typeof v=="bigint"?v+"n":Object.is(v,-0)?"-0":typeof v=="string"?JSON.stringify(v):String(v),' +
  'T=f=>{try{return S(f())}catch(e){return"!"+e.name+":"+e.message}};\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const T = body => `T(()=>${body})`;

// Escapa tudo fora do ASCII imprimível, para a fonte não carregar caractere de controle nem separador de linha.
const lit = text => JSON.stringify(text).replace(/[^\x20-\x7e]/g, c => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));

// Gerador pseudoaleatório determinístico (mulberry32) semeado por um rótulo: o valor de cada entrada depende só do rótulo
// (hash SHA-1) e nunca da ordem em que as outras foram geradas nem da posição no laço.
const hashInt = label => parseInt(require("crypto").createHash("sha1").update(label).digest("hex").slice(0, 8), 16);
const hashPick = (list, label) => list[hashInt(label) % list.length];
const keyedRand = label => {
  let seed = hashInt(label) | 0;
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};
// BigInt de exatamente `bits` bits, com o bit alto ligado.
const randBits = (bits, label) => {
  const rand = keyedRand(label + ":" + bits);
  let v = 1n;
  for (let i = 1; i < bits; i++) v = (v << 1n) | (rand() < 0.5 ? 0n : 1n);
  return v;
};
const lib = v => (v < 0n ? "-0x" + (-v).toString(16) : "0x" + v.toString(16)) + "n";
const wrap = v => "(" + lib(v) + ")";

// ---- 1. asIntN / asUintN.
const widths = [0, 1, 7, 8, 31, 32, 33, 63, 64, 65, 127, 128];
const boundaryFor = b => {
  const out = new Set(["0n", "1n", "-1n", "2n", "-2n"]);
  const p = 2n ** BigInt(b);
  const h = b > 0 ? 2n ** BigInt(b - 1) : 0n;
  for (const d of [-2n, -1n, 0n, 1n, 2n]) {
    for (const base of [p, h]) {
      out.add(lib(base + d));
      out.add(lib(-base + d));
      out.add(lib(base * 3n + d));
    }
  }
  return [...out];
};
for (const b of widths) {
  for (const v of boundaryFor(b)) add(T(`BigInt.asIntN(${b},${v})`), T(`BigInt.asUintN(${b},${v})`));
}
for (const b of [200, 256]) for (const v of boundaryFor(b).slice(0, 12)) add(T(`BigInt.asIntN(${b},${v})`), T(`BigInt.asUintN(${b},${v})`));
for (const v of ["0n", "1n", "-1n", "255n", "-255n", "2n**64n", "-(2n**64n)", "2n**200n+1n", "-(2n**200n)-1n"]) {
  add(T(`BigInt.asIntN(2**53-1,${v})`), T(`BigInt.asUintN(2**53-1,${v})`));
}
const badBits = ["-1", "-2", "-(2**53)", "2**53", "2**53+1", "2**64", "1e300", "-1e300", "Infinity", "-Infinity", "NaN", "-0", "0.5", "1.9", "64.9", "-0.9", "'8'", "' 8 '", "'x'", "''", "undefined", "null", "true", "false", "[]", "[8]", "({})", "({valueOf(){return 8}})",
  "({valueOf(){return -1}})", "8n", "-1n", "0n", "Symbol()", "new Number(8)", "new String('8')", "'0x10'", "'1e1'", "2**53-1", "2**53-2"];
for (const b of badBits) {
  for (const v of ["0n", "1n", "-1n", "255n", "-(2n**63n)"]) add(T(`BigInt.asIntN(${b},${v})`), T(`BigInt.asUintN(${b},${v})`));
}
for (const v of ["5", "5.5", "'5'", "'x'", "''", "true", "false", "null", "undefined", "NaN", "Infinity", "Symbol()", "[]", "[5]", "({})", "({valueOf(){return 5n}})", "({valueOf(){return 5}})", "Object(5n)", "Object(-5n)", "'0x10'", "' 12 '", "'1n'"]) {
  add(T(`BigInt.asIntN(8,${v})`), T(`BigInt.asUintN(8,${v})`), T(`BigInt.asIntN(64,${v})`), T(`BigInt.asUintN(64,${v})`));
}
add(T("BigInt.asIntN()"), T("BigInt.asUintN()"), T("BigInt.asIntN(8)"), T("BigInt.asUintN(8)"), T("BigInt.asIntN(0n)"), T("new BigInt.asIntN(8,1n)"), T("BigInt.asIntN.call(null,8,300n)"), T("BigInt.asUintN.call(1,8,-1n)"));

// ---- 2. Operadores com BigInt de 1 a 300 bits.
const sizes = [1, 2, 3, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 52, 53, 54, 63, 64, 65, 96, 100, 127, 128, 129, 191, 192, 200, 255, 256, 257, 300];
const binary = ["+", "-", "*", "/", "%", "&", "|", "^"];
for (const sa of sizes) {
  const sb = hashPick(sizes, "sb:" + sa);
  const sc = hashPick(sizes, "sc:" + sa);
  const a = randBits(sa, "a");
  const b = randBits(sb, "b" + sa);
  const c = randBits(sc, "c" + sa);
  for (const [x, y] of [[a, b], [-a, b], [a, -b], [-a, -b], [b, c], [-c, a]]) {
    for (const op of binary) add(T(`${wrap(x)}${op}${wrap(y)}`));
  }
  for (const x of [a, -a]) {
    add(T(`~${wrap(x)}`), T(`-${wrap(x)}`), T(`+${wrap(x)}`), T(`${wrap(x)}+1n`), T(`${wrap(x)}-1n`), T(`${wrap(x)}*0n`), T(`${wrap(x)}*-1n`), T(`${wrap(x)}/1n`), T(`${wrap(x)}%1n`), T(`${wrap(x)}&0n`),
      T(`${wrap(x)}|0n`), T(`${wrap(x)}^${wrap(x)}`), T(`${wrap(x)}-${wrap(x)}`), T(`${wrap(x)}/${wrap(x)}`), T(`${wrap(x)}%${wrap(x)}`), T(`${wrap(x)}&-1n`), T(`${wrap(x)}|-1n`), T(`${wrap(x)}^-1n`),
      T(`${wrap(x)}/0n`), T(`${wrap(x)}%0n`), T(`${wrap(x)}>>>0n`), T(`${wrap(x)}>>>1n`), T(`1n>>>${wrap(x)}`), T(`${wrap(x)}/0`), T(`${wrap(x)}+1`), T(`${wrap(x)}*1.5`), T(`${wrap(x)}**-1n`), T(`${wrap(x)}**0n`), T(`${wrap(x)}**1n`));
    for (const s of [0, 1, 2, 7, 8, 31, 32, 33, 63, 64, 65, 127, 128, 200]) add(T(`${wrap(x)}<<${s}n`), T(`${wrap(x)}>>${s}n`), T(`${wrap(x)}<<-${s}n`), T(`${wrap(x)}>>-${s}n`));
    for (const e of [2, 3, 5, 10]) if (sa <= 100 || e <= 3) add(T(`${wrap(x)}**${e}n`));
  }
  const small = hashPick([2n, -2n, 3n, -3n, 7n, 10n, -10n], "small:" + sa);
  for (const e of [0n, 1n, 2n, 3n, 10n, 31n, 64n, 65n, 100n]) add(T(`${wrap(small)}**${e}n`));
  add(T(`${wrap(a)}**${wrap(randBits(6, "exp" + sa))}`));
}
add(T("0n**0n"), T("0n**1n"), T("0n**-1n"), T("1n**-1n"), T("(-1n)**-1n"), T("2n**-1n"), T("(-2n)**-3n"), T("1n**(2n**100n)"), T("(-1n)**(2n**100n)"), T("(-1n)**(2n**100n+1n)"), T("0n**(2n**100n)"), T("2n**(2n**100n)"),
  T("2n**(2n**64n)"), T("2n**1073741824n"), T("2n**0n"), T("(2n**64n)**2n"), T("2n**300n"), T("(-2n)**301n"), T("3n**200n"), T("5n**-0n"), T("0n/0n"), T("0n%0n"), T("-0n"), T("-(-0n)"), T("1n/0n"), T("-1n/0n"),
  T("1n%0n"), T("5n>>>1n"), T("(-5n)>>>0n"), T("5n>>>5"), T("0n>>>0n"), T("1n<<(2n**64n)"), T("0n<<(2n**64n)"), T("1n>>(2n**64n)"), T("-1n>>(2n**64n)"), T("1n<<-(2n**64n)"), T("-1n<<-(2n**64n)"), T("1n<<1073741824n"),
  T("1n+1"), T("1n-1"), T("1n*1"), T("1n/1"), T("1n%1"), T("1n**1"), T("1n<<1"), T("1n>>1"), T("1n&1"), T("1n|1"), T("1n^1"), T("~1.5"), T("+1n"), T("1n+'1'"), T("'1'+1n"), T("1n+undefined"), T("1n+null"), T("1n+true"), T("1n+Symbol()"),
  T("1n+[]"), T("1n+{}"), T("1n-'1'"), T("1n*'2'"), T("1n+Object(2n)"), T("Object(2n)*Object(3n)"), T("++Object(1n)"), T("(()=>{var a=2n;a++;a--;++a;return a})()"), T("(()=>{var a=2n;return [a++,a--,++a,--a,a**=3n,a<<=2n,a>>=1n,a&=7n,a|=8n,a^=1n,a%=5n,a/=2n,a*=3n,a-=1n,a+=2n].join()})()"),
  T("(()=>{var a=-(2n**64n);a++;return a})()"), T("(()=>{var a=2n**64n;a--;return a})()"), T("(()=>{var a=0n;a--;return a})()"), T("(()=>{var a=-1n;a++;return a})()"));

// ---- 3. BigInt() de strings.
const core = ["0", "1", "12", "-12", "+12", "00012", "-0", "+0", "0x1f", "0X1F", "0o17", "0O17", "0b101", "0B101", "-0x1", "+0x1", "0x", "0b", "0o", "0b2", "0o8", "0xg", "1n", "1.5", "1e3", "1_000", "Infinity", "NaN", "12 3", "--1", "+-1", "- 1", "+ 1",
  "9007199254740993", "-9007199254740993", "340282366920938463463374607431768211456", "-340282366920938463463374607431768211456", "0x" + "f".repeat(80), "0b" + "1".repeat(130), "0o" + "7".repeat(45),
  "1" + "0".repeat(120), "0.0", ".5", "5.", "0x0", "0b0", "0o0", "0x-1", "-0b1", "+0o7", "1e", "e1", "0xe", "0Xa", "0xA", "0b1_0", "0_1", "1 ", " 1", "1\u0000", "a", "٣", "１２", "00", "000x1", "0x00ff", "0b00101", "0o0017", "1,000", "١٢"];
const spaces = ["", " ", "  ", "\n", "\t", "\r", "\v", "\f", " ", "﻿", " ", " ", " ", "　", "᠎", "​"];
for (const s of core) {
  add(T(`BigInt(${lit(s)})`), T(`typeof BigInt(${lit(s)})`));
}
for (const s of ["12", "-12", "0x1f", "0b11", "0o7", "+5", "", "0", "1.5", "0x"]) {
  for (const sp of spaces) {
    if (sp === "") continue;
    add(T(`BigInt(${lit(sp + s)})`), T(`BigInt(${lit(s + sp)})`), T(`BigInt(${lit(sp + s + sp)})`));
  }
  add(T(`BigInt(${lit(" " + s + "\n")})`));
}
for (const s of ["", " ", "\n", " ", " ", "﻿"]) add(T(`BigInt(${lit(s)})`), T(`BigInt(${lit(s)})===0n`), T(`${lit(s)}==0n`));
// Mesmo texto por ToNumeric implícito, que vira o mesmo parse.
for (const s of ["12", "0x1f", "1.5", "x", "", " 7 ", "-0x1", "1n"]) add(T(`${lit(s)}==1n`), T(`${lit(s)}<1n`), T(`1n>${lit(s)}`), T(`${lit(s)}==12n`), T(`${lit(s)}==31n`), T(`BigInt.asIntN(8,${lit(s)})`));
// Parse que cruza a fronteira de tamanho.
for (const bits of [31, 32, 33, 53, 63, 64, 65, 127, 128, 129, 255, 256, 300]) {
  const v = 2n ** BigInt(bits);
  for (const d of [-1n, 0n, 1n]) {
    const n = v + d;
    add(T(`BigInt(${lit(n.toString())})`), T(`BigInt(${lit("-" + n.toString())})`), T(`BigInt(${lit("0x" + n.toString(16))})`), T(`BigInt(${lit("0b" + n.toString(2))})`), T(`BigInt(${lit("0o" + n.toString(8))})`),
      T(`BigInt(${lit(" +" + n.toString() + " ")})`), T(`BigInt(${lit("0x" + n.toString(16) + "g")})`), T(`BigInt(${lit(n.toString() + "n")})`), T(`BigInt(${lit(n.toString() + ".0")})`));
  }
}
add(T("BigInt('0x' + 'f'.repeat(200))"), T("BigInt('1' + '0'.repeat(300))"), T("BigInt('-' + '9'.repeat(150))"), T("BigInt('0b' + '1'.repeat(300)) === 2n**300n - 1n"));

// ---- 4. BigInt() de números e outros valores.
const numbers = ["0", "-0", "1", "-1", "1.5", "-1.5", "0.1", "0.5", "-0.5", "1e-7", "5e-324", "NaN", "Infinity", "-Infinity", "2**31", "2**32", "2**53", "2**53+2", "-(2**53)", "2**63", "2**64", "-(2**63)", "2**100", "2**1023", "Number.MAX_VALUE",
  "Number.MIN_VALUE", "Number.MAX_SAFE_INTEGER", "Number.MIN_SAFE_INTEGER", "Number.EPSILON", "1e21", "1e22", "-1e21", "1e300", "123456789.123", "9007199254740993", "4.5e15", "4.5e15+0.5", "2**53-0.5", "1/3", "Math.PI", "-Math.E", "1e15+0.3", "0.1+0.2",
  "100", "1e2", "0xff", "0b11", "1_0", "2**-1", "2**-1074", "-(2**1024)", "(2**53)*1.5"];
for (const x of numbers) add(T(`BigInt(${x})`), T(`typeof BigInt(${x})`), T(`BigInt(-(${x}))`), T(`BigInt(Object(${x}))`), T(`BigInt(String(${x}))`));
for (const x of ["true", "false", "null", "undefined", "Symbol()", "[]", "[7]", "[7,8]", "['9']", "({})", "({valueOf(){return 3}})", "({valueOf(){return 3.5}})", "({valueOf(){return 3n}})", "({toString(){return '0x10'}})",
  "({[Symbol.toPrimitive](){return 9n}})", "({[Symbol.toPrimitive](){return 9.5}})", "({[Symbol.toPrimitive](h){return h}})", "Object(5n)", "new Number(7)", "new Number(7.5)", "new String('8')", "new String('8.5')", "new Boolean(true)", "()=>{}", "new Date(5)", "[[]]", "['']", "[' 5 ']", "[1.5]",
  "new Array(1)", "{valueOf(){return 3}}.valueOf()", "10n", "-10n", "2n**100n"]) add(T(`BigInt(${x})`), T(`typeof BigInt(${x})`));
add(T("BigInt()"), T("new BigInt(1)"), T("new BigInt()"), T("BigInt(1,2)"), T("BigInt.length"), T("BigInt.name"), T("BigInt.call(null,5)"), T("BigInt.apply(undefined,['6'])"), T("Reflect.construct(BigInt,[1])"), T("Reflect.apply(BigInt,null,[1.5])"),
  T("[1,2.5,3].map(BigInt)"), T("['1','2','x'].map(BigInt)"), T("[1,2,3].map(BigInt).join()"), T("Object(1n) instanceof BigInt"), T("typeof Object(1n)"), T("BigInt(Object(1n))===1n"));

// ---- 5. toString(radix).
const valuesForRadix = [];
for (const s of [1, 2, 5, 8, 31, 32, 33, 53, 63, 64, 65, 100, 128, 200, 300]) valuesForRadix.push(randBits(s, "radix"));
valuesForRadix.push(0n, 1n, 35n, 36n, 255n, 256n, 2n ** 64n - 1n, 2n ** 64n, 10n ** 30n);
const radixes = [2, 3, 4, 7, 8, 10, 11, 16, 20, 32, 35, 36];
// Um terço dos valores percorre os 35 radixes, escolhido por hash do valor (`sampleByHash`).
const allRadixValues = new Set(sampleByHash(valuesForRadix, Math.ceil(valuesForRadix.length / 3), v => v.toString(16)));
valuesForRadix.forEach(v => {
  for (const x of [v, -v]) {
    if (x === 0n && v !== 0n) continue;
    for (const r of radixes) add(T(`${wrap(x)}.toString(${r})`));
    add(T(`${wrap(x)}.toString()`), T(`String(${wrap(x)})`), T(`${wrap(x)}.toString(undefined)`), T(`${wrap(x)}.toLocaleString('en-US')`), T(`${wrap(x)}.valueOf()`), T("`${" + wrap(x) + "}`"), T(`""+${wrap(x)}`), T(`JSON.stringify([String(${wrap(x)})])`));
  }
  if (allRadixValues.has(v)) for (let r = 2; r <= 36; r++) add(T(`${wrap(v)}.toString(${r})`), T(`${wrap(-v)}.toString(${r})`));
});
for (const r of ["1", "0", "37", "-1", "100", "1.5", "2.9", "36.9", "37.1", "NaN", "Infinity", "-Infinity", "null", "undefined", "'16'", "'x'", "''", "true", "false", "[]", "[16]", "({})", "({valueOf(){return 8}})", "16n", "Symbol()", "-0", "0.9", "1.9", "'0x10'", "2**32+2", "2**53"]) {
  for (const v of ["0n", "255n", "-255n", "2n**70n"]) add(T(`(${v}).toString(${r})`));
}
add(T("BigInt.prototype.toString.call(1)"), T("BigInt.prototype.toString.call('x')"), T("BigInt.prototype.toString.call({})"), T("BigInt.prototype.toString.call(Object(255n),16)"), T("BigInt.prototype.toString.call(null)"), T("BigInt.prototype.toString.call(undefined)"),
  T("BigInt.prototype.toString.length"), T("BigInt.prototype.valueOf.call(1)"), T("BigInt.prototype.valueOf.call(Object(7n))"), T("BigInt.prototype.toLocaleString.call(5)"), T("(-0n).toString(2)"), T("(0n).toString(36)"), T("(35n).toString(36)"), T("(-35n).toString(36)"),
  T("(2n**64n).toString(2).length"), T("(-(2n**64n)).toString(2).length"), T("(10n**100n).toString(16)"), T("(2n**300n).toString(36)"), T("BigInt('0x'+(7n**90n).toString(16))===7n**90n"), T("BigInt((9n**100n).toString())===9n**100n"));

// ---- 6. Comparação com Number e string.
const bigs = ["0n", "1n", "-1n", "2n", "10n", "-10n", "255n", "2n**31n", "2n**32n", "2n**53n", "2n**53n+1n", "-(2n**53n)-1n", "2n**63n", "2n**64n", "2n**64n+1n", "-(2n**64n)", "2n**100n", "2n**1023n", "2n**1024n", "-(2n**1024n)", "2n**2000n", "9007199254740993n", "9007199254740995n"];
const others = ["0", "-0", "1", "-1", "0.5", "-0.5", "1.5", "10", "-10", "10.5", "255", "2**31", "2**32", "2**53", "2**53+2", "-(2**53)", "2**63", "2**64", "2**100", "2**1023", "Number.MAX_VALUE", "-Number.MAX_VALUE", "Number.MIN_VALUE", "Infinity", "-Infinity", "NaN", "9007199254740992", "9007199254740994", "1e21", "1e300",
  "'0'", "''", "' '", "'1'", "' 1 '", "'-1'", "'+1'", "'10'", "'0x0a'", "'0b11'", "'0o7'", "'1e1'", "'1.5'", "'1n'", "'x'", "'Infinity'", "'-Infinity'", "'NaN'", "'255'", "'2'.repeat(25)", "'9007199254740993'", "'9007199254740995'", "'18446744073709551616'", "'-18446744073709551616'", "'1_0'", "'\\n10\\t'", "true", "false", "null", "undefined", "[]", "[1]", "[10]", "({})", "Object(1n)", "Object(10)"];
for (const a of bigs) {
  for (const b of others) {
    add(T(`[(${a})<(${b}),(${a})>(${b}),(${a})<=(${b}),(${a})>=(${b}),(${a})==(${b}),(${a})!=(${b}),(${a})===(${b})].join()`));
  }
}
for (const a of bigs.slice(0, 12)) for (const b of others) add(T(`[(${b})<(${a}),(${b})>(${a}),(${b})<=(${a}),(${b})>=(${a}),(${b})==(${a})].join()`));
add(T("[1n<NaN,1n>NaN,1n<=NaN,1n>=NaN,1n==NaN,1n!=NaN].join()"), T("[1n<Infinity,1n>-Infinity,1n>Infinity,1n<-Infinity,2n**1024n<Infinity,2n**1024n==Infinity].join()"), T("[0n==-0,0n===-0,0n==0,0n==false,1n==true,2n==true,0n==''].join()"),
  T("[Object.is(0n,-0n),[0n].includes(-0),[1n].includes(1),[1n].indexOf(1),new Set([1n,1n,1,Object(1n)]).size].join()"), T("[3n,1n,10n,-2n].sort().join()"), T("[3n,1n,10n,-2n].sort((a,b)=>a<b?-1:a>b?1:0).join()"),
  T("[3n,1,2n,0.5,-1n].sort((a,b)=>a<b?-1:a>b?1:0).join()"), T("Math.max(1n,2n)"), T("Math.max(...[1n,2n])"), T("[1n,2n,3n].reduce((a,b)=>a+b)"), T("[1n,2n,3n].reduce((a,b)=>a>b?a:b)"));

// ---- Execução.
const baseSet = new Set(knownPrograms("bigint_grid_bun.tsv", name => /bigint/i.test(name) && name !== "bigint_grid_bun.tsv" && name !== "bigint.tsv"));
const seen = new Set();
exprs.splice(0, exprs.length, ...exprs.filter(p => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSet.has(source)) { dup++; continue; }
  jobs.push({ expr, source });
}

const run = job =>
  new Promise(resolve => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 8000, killSignal: "SIGKILL" });
    let out = "";
    let err = "";
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", (code, signal) => resolve(code === 0 ? { ok: true, out } : { ok: false, err: signal ? "timeout " + signal : err }));
    child.stdin.on("error", () => {});
    child.stdin.end(job.source);
  });

async function main() {
  if (process.env.COUNT) { console.error(jobs.length); return; }
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await run(jobs[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  jobs.forEach((job, i) => {
    const r = results[i];
    if (!r.ok) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + " " + r.err.slice(0, 120) + "\n"); return; }
    const line = JSON.stringify(job.source) + "\t" + JSON.stringify(r.out);
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || /[–—]/.test(line) || /\\u201[34]/.test(line)) {
      dropped++;
      process.stderr.write("caminho, marca ou travessão: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(line);
  });
  process.stdout.write(emitFactoredLines("bigint_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
