// Gera tests/golden/bigint_symbol_bun.tsv: BigInt, Symbol e conversões abstratas, medido no bun 1.4.2.
// Cobre o construtor BigInt, asIntN/asUintN, toString com radix, operadores mistos com Number, comparações, shifts,
// expoente negativo, divisão por zero, literais, parse de string, JSON, Symbol.toPrimitive, descrição, for/keyFor,
// well-knowns, conversões implícitas que lançam, Object(sym), chaves por símbolo em ownKeys/assign/spread e uma grade
// de valores por operadores de ToNumber/ToString/ToPrimitive/ToPropertyKey com o log da ordem de valueOf/toString.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo (a tabela estática do JSC reifica por ordem de acesso), em paralelo.
// Uso: bun scripts/gen-bigint-symbol-golden.js > tests/golden/bigint_symbol_bun.tsv
const { emitFactoredLines, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'var S=v=>typeof v=="symbol"?String(v):typeof v=="bigint"?v+"n":Object.is(v,-0)?"-0":typeof v=="string"?JSON.stringify(v):String(v),' +
  'T=f=>{try{return S(f())}catch(e){return"!"+e.name+":"+e.message}};\n';
// Ajudante de log: Q (valueOf/toString com retorno próprio), TP (Symbol.toPrimitive), U (T com o log na frente).
const LOGGER =
  'var L=[],Q=(a,v,s)=>({valueOf(){L.push(a+"v");return v},toString(){L.push(a+"s");return s}}),' +
  'TP=(a,b)=>({[Symbol.toPrimitive](h){L.push(a+h);return b}}),U=f=>{var r=T(f);return L.join()+">"+r};\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const T = body => `T(()=>${body})`;

// ---- 1. Construtor BigInt e conversão.
const ctorInputs = [
  "0", "1", "-1", "-0", "0.5", "1.5", "NaN", "Infinity", "-Infinity", "2**53", "2**64", "1e21", "1e300", "-1e21", "Number.MAX_SAFE_INTEGER", "Number.MAX_VALUE",
  "''", "' '", "'0'", "'-0'", "'12'", "'-12'", "'+12'", "' 12 '", "'12 '", "'1_0'", "'0x1f'", "'0X1F'", "'0b11'", "'0o17'", "'-0x1'", "'+0x1'", "'1n'", "'1.0'", "'1e3'",
  "'.5'", "'5.'", "'abc'", "'Infinity'", "'NaN'", "'\\n7\\t'", "'\\u00a07'", "'9007199254740993'", "'123456789012345678901234567890'", "'-123456789012345678901234567890'",
  "'0x'", "'0b'", "'0b2'", "'00012'", "'1 2'", "'--1'", "'\\u2028 5'",
  "true", "false", "null", "undefined", "1n", "-1n", "0n", "2n**100n", "Symbol()", "[]", "[5]", "[5,6]", "['7']", "{}", "{valueOf(){return 9}}", "{valueOf(){return 9n}}",
  "{valueOf(){return '10'}}", "{toString(){return '11'}}", "{valueOf(){return 1.5}}", "{valueOf(){return {}},toString(){return '3'}}", "Object(5n)", "Object(5)", "Object('8')", "new Date(5)",
  "()=>1", "{[Symbol.toPrimitive](){return 4}}", "{[Symbol.toPrimitive](h){return h}}", "{[Symbol.toPrimitive](){return 4n}}", "{[Symbol.toPrimitive]:1}",
];
for (const v of ctorInputs) add(T(`BigInt(${v})`), T(`typeof BigInt(${v})`), T(`BigInt.asIntN(8,${v})`), T(`BigInt.asUintN(8,${v})`), T(`new BigInt(${v})`));
add(
  T("BigInt.length+BigInt.name"), T("typeof BigInt"), T("BigInt()"), T("BigInt.prototype.constructor===BigInt"), T("Object.getPrototypeOf(1n)===BigInt.prototype"),
  T("Object.prototype.toString.call(1n)"), T("Object.prototype.toString.call(Object(1n))"), T("BigInt.prototype[Symbol.toStringTag]"), T("Object.getOwnPropertyNames(BigInt).join()"),
  T("Object.getOwnPropertyNames(BigInt.prototype).join()"), T("BigInt.asIntN.length+BigInt.asUintN.length"), T("BigInt.asIntN.name+BigInt.asUintN.name"), T("typeof Object(1n)"),
  T("Object(1n)==1n"), T("Object(1n)===1n"), T("Object(1n)+1n"), T("1n instanceof BigInt"), T("Object(1n) instanceof BigInt"), T("BigInt.prototype.valueOf.call(1n)"),
  T("BigInt.prototype.valueOf.call(1)"), T("BigInt.prototype.valueOf.call(Object(3n))"), T("BigInt.prototype.toString.call(1)"), T("BigInt.prototype.toString.call(Object(255n),16)"),
  T("BigInt.prototype.toLocaleString.call(1234567n)"), T("(1234567n).toLocaleString('en-US')"), T("(1234567n).toLocaleString('de-DE')"), T("(-5n).toLocaleString()"),
  T("1n.constructor===BigInt"), T("(5n).constructor.name"), T("1n.hasOwnProperty('x')"), T("Object.keys(Object(1n)).length"), T("Object.isFrozen(1n)"), T("Object.isFrozen(Object(1n))"),
  T("{var a=1n;a.x=5;return a.x}"), T("(()=>{'use strict';var a=1n;a.x=5})()"), T("Number(2n**64n)"), T("Number(-(2n**1100n))"), T("Number(2n**1023n)"), T("Number(2n**1024n)"),
  T("Number(9007199254740993n)"), T("Number(9007199254740995n)"), T("Number(18014398509481985n)"), T("parseInt('12n')"), T("parseInt(12n)"), T("parseFloat(12n)"), T("Number.parseInt(2n**70n)"),
  T("Math.max(1n)"), T("Math.abs(-1n)"), T("Math.floor(1n)"), T("Math.sqrt(4n)"), T("isNaN(1n)"), T("Number.isNaN(1n)"), T("Number.isInteger(1n)"), T("Number.isSafeInteger(1n)"),
  T("isFinite(1n)"), T("Number.isFinite(1n)"), T("[1n,2n].includes(1n)"), T("[1n].indexOf(1n)"), T("[NaN].includes(NaN)+','+[0n].includes(-0)"), T("new Set([1n,1n,1]).size"),
  T("new Map([[1n,'a']]).get(1n)"), T("new Map([[1n,'a']]).get(1)"), T("Object.is(0n,-0n)"), T("Object.is(1n,1n)"), T("Object.is(0n,0)"), T("[3n,1n,2n].sort()+''"),
  T("[3n,1n,10n].sort((a,b)=>a<b?-1:a>b?1:0)+''"), T("[3n,1n,10n].sort((a,b)=>a-b)+''"), T("[10n,9n,1n].sort()+''"), T("[1,2n,3].map(x=>typeof x).join()"),
  T("new Array(3n)"), T("new Array(3n).length"), T("[1,2,3][1n]"), T("({1:'a'})[1n]"), T("({[1n]:'a'})[1]"), T("Object.keys({[2n**64n]:1})[0]"), T("'abc'[1n]"),
  T("'abc'.at(1n)"), T("'abc'.slice(1n)"), T("'abc'.repeat(2n)"), T("[1,2,3].at(1n)"), T("[1,2,3].slice(1n)"), T("Array(2).fill(0,1n)"), T("new Uint8Array(2n)"),
  T("new BigInt64Array([1n,2n]).join()"), T("new BigInt64Array([1])"), T("new BigUint64Array([-1n])[0]"), T("new BigInt64Array([2n**63n])[0]"), T("new BigUint64Array(1).fill(2n**64n+3n)[0]"),
  T("new BigInt64Array(2)[0]"), T("typeof new BigInt64Array(1)[0]"), T("new BigInt64Array([1n]).map(x=>x*2n)[0]"), T("new Uint8Array([1n])"), T("new BigInt64Array(new Uint8Array(1))"),
  T("BigInt64Array.BYTES_PER_ELEMENT+','+BigUint64Array.BYTES_PER_ELEMENT"), T("new DataView(new ArrayBuffer(8)).getBigInt64(0)"),
  T("{var d=new DataView(new ArrayBuffer(8));d.setBigUint64(0,2n**64n-1n);return d.getBigInt64(0)+','+d.getUint8(7)}"),
  T("{var d=new DataView(new ArrayBuffer(8));d.setBigInt64(0,1n,true);return d.getUint8(0)+','+d.getUint8(7)}"), T("new DataView(new ArrayBuffer(8)).setBigInt64(0,1)"),
  T("Atomics.add(new BigInt64Array(new SharedArrayBuffer(8)),0,2n)"), T("Atomics.add(new BigInt64Array(new SharedArrayBuffer(8)),0,2)"),
);

// ---- 2. asIntN / asUintN em grade.
const bitsList = ["0", "1", "2", "3", "8", "31", "32", "63", "64", "65", "128", "-1", "2**53-1", "2**53", "'8'", "1.9", "NaN", "undefined", "null", "true", "{valueOf(){return 4}}", "8n", "Infinity"];
const bigVals = ["0n", "1n", "-1n", "127n", "128n", "255n", "256n", "-128n", "-129n", "2n**63n", "2n**64n-1n", "-(2n**63n)", "-(2n**63n)-1n", "2n**200n+5n", "-(2n**200n)+5n", "5", "'5'", "undefined"];
for (const b of bitsList) for (const v of bigVals) add(T(`BigInt.asIntN(${b},${v})`), T(`BigInt.asUintN(${b},${v})`));

// ---- 3. toString com radix.
const radixes = ["undefined", "2", "3", "7", "8", "10", "16", "32", "36", "1", "37", "0", "-1", "'16'", "16.9", "NaN", "null", "{valueOf(){return 2}}", "2n", "Infinity", "true"];
const toStringVals = ["0n", "1n", "-1n", "255n", "-255n", "35n", "36n", "2n**64n", "-(2n**64n)", "10n**30n", "123456789012345678901234567890n", "Object(31n)", "0x7fffffffffffffffn", "-0n"];
for (const r of radixes) for (const v of toStringVals) add(T(`(${v}).toString(${r})`));
for (const v of toStringVals) add(T(`String(${v})`), T(`''+(${v})`), T("`${" + v + "}`"), T(`(${v}).toString()`), T(`(${v}).toLocaleString()`), T(`JSON.stringify([${v}.toString()])`), T(`[${v}]+''`), T(`(${v}).valueOf()`));

// ---- 4. Operadores mistos em grade.
const binOps = ["+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>", ">>>", "<", ">", "<=", ">=", "==", "!=", "===", "!=="];
const gridVals = ["0n", "1n", "-1n", "5n", "-7n", "2n**64n", "3", "2.5", "-0", "'3'", "'x'", "true", "null", "NaN"];
for (const a of gridVals) for (const b of gridVals) for (const op of binOps) add(T(`(${a})${op}(${b})`));

// ---- 5. Comparações BigInt contra string, Number grande e especiais.
const cmpA = ["10n", "-10n", "0n", "2n**64n", "9007199254740993n", "9007199254740992n", "1n"];
const cmpB = ["'10'", "' 10 '", "'1e1'", "'0x0a'", "'x'", "''", "' '", "'10n'", "'10.5'", "'-10'", "'-0'", "'1_0'", "'18446744073709551616'", "10", "10.5", "-10", "2**64", "2**53", "2**53+2", "Infinity", "-Infinity", "NaN", "0", "-0", "1", "true", "null", "undefined", "[10]", "[]", "{}", "Object(10n)", "Object(10)", "'9007199254740993'"];
for (const a of cmpA) for (const b of cmpB) add(T(`[(${a})<(${b}),(${a})>(${b}),(${a})<=(${b}),(${a})>=(${b}),(${a})==(${b}),(${a})!=(${b}),(${a})===(${b})].join()`), T(`[(${b})<(${a}),(${b})>=(${a}),(${b})==(${a})].join()`));
add(
  T("[1n<2,2<1n,1n<=1,1n>=1,2n>1.5,1n<1.5,1n<2.5,-1n>-1.5,-1n<-0.5].join()"), T("[1n<Infinity,1n>-Infinity,1n<NaN,1n>NaN,1n==NaN].join()"), T("[0n==-0,0n===-0,0n==0,0n==false,1n==true,2n==true,0n==''].join()"),
  T("[0n==null,0n==undefined,0n<undefined,0n>=null,0n<=null].join()"), T("[1n==Object(1n),Object(1n)==Object(1n),Object(1n)<Object(2n)].join()"), T("1n<'2'"), T("'3'<2n"), T("'x'<2n"), T("2n<'x'"),
  T("[2n**64n==2**64,2n**64n+1n==2**64,2n**64n+1n>2**64,2n**64n<2**64+4096].join()"), T("[3n]==3n"), T("'3'==3n"), T("3n=='3.0'"), T("3n==Symbol()"), T("Symbol()<3n"), T("3n<{}"),
  T("[1n,2n,3n].indexOf(2)"), T("[1n,2n].includes(2)"), T("[1,'1',1n].filter(x=>x==1n).length"), T("[1,'1',1n].filter(x=>x===1n).length"), T("1n?'t':'f'"), T("0n?'t':'f'"), T("!0n"), T("!1n"), T("!!-0n"),
  T("1n&&2n"), T("0n||5n"), T("0n??5n"), T("typeof 1n"), T("typeof Object(1n)"), T("typeof(0n)"), T("void 1n"), T("1n,2n"), T("[0n].some(Boolean)"), T("Boolean(0n)+','+Boolean(Object(0n))"),
  T("Math.max(1n,2n)"), T("Math.min()"), T("Math.hypot(3n)"), T("Math.trunc(1n)"), T("+1n"), T("+Object(1n)"), T("-Object(1n)"), T("~Object(1n)"), T("Number(Object(1n))"),
);

// ---- 6. Unários, update e atribuição composta.
const unVals = ["0n", "1n", "-1n", "2n**64n", "-(2n**64n)", "5", "-0", "'7'", "true", "null", "undefined", "NaN", "{}", "[]", "[2]", "Symbol()", "Object(4n)", "Object(4)"];
for (const v of unVals) {
  add(T(`-(${v})`), T(`+(${v})`), T(`~(${v})`), T(`!(${v})`), T(`typeof(${v})`), T(`(${v})+(${v})`), T(`{var x=${v};x++;return [x,typeof x].join()}`), T(`{var x=${v};var y=x--;return [y,x].join()}`),
    T(`{var x=${v};++x;return x}`), T(`{var x=${v};x+=1n;return x}`), T(`{var x=${v};x**=2n;return x}`), T(`{var x=${v};x<<=1n;return x}`), T(`{var x=${v};x>>>=0n;return x}`),
    T(`{var x=${v};x%=3n;return x}`), T(`{var x=${v};x/=2n;return x}`), T(`{var x=${v};x-=1;return x}`), T(`{var x=${v};x&=1n;return x}`), T(`{var x=${v};x??=9n;return x}`), T(`{var x=${v};x||=9n;return x}`),
    T(`{var o={p:${v}};o.p++;return o.p}`), T(`{var a=[${v}];a[0]*=2n;return a[0]}`), T(`(${v})**0n`), T(`(${v})**1n`), T(`(${v})**-0n`), T(`(${v})/1n`), T(`(${v})%1n`));
}
add(
  T("-(2n**63n)"), T("-0n"), T("Object.is(-0n,0n)"), T("0n*-1n"), T("-0n===0n"), T("(-0n).toString()"), T("1/Number(-0n)"), T("~0n"), T("~-1n"), T("~(2n**64n)"), T("-(-(2n**64n))"),
  T("{var x=9007199254740993n;x++;return x}"), T("{var x=2n**64n;x--;return x}"), T("{var i=0n;for(;i<3n;i++);return i}"), T("{var s=0n;for(var i=1n;i<=20n;i++)s*=1n,s+=i;return s}"),
  T("{var f=1n;for(var i=1n;i<=25n;i++)f*=i;return f}"), T("{var a=0n,b=1n;for(var i=0;i<100;i++)[a,b]=[b,a+b];return a}"), T("10n**100n"), T("-(10n**100n)"), T("(-3n)**3n"), T("(-3n)**2n"),
  T("2n**1000n%1000007n"), T("0n**0n"), T("(-1n)**(2n**70n)"), T("(-1n)**(2n**70n+1n)"), T("1n**(2n**70n)"), T("2n**(2n**70n)"), T("0n**(2n**70n)"), T("2n**(2n**30n)"), T("2n**30000000n"),
);

// ---- 7. Shifts.
const shiftVals = ["1n", "-1n", "5n", "-5n", "255n", "-256n", "2n**64n", "-(2n**64n)", "2n**64n+1n", "-(2n**64n)-1n", "0n"];
const shiftBy = ["0n", "1n", "3n", "63n", "64n", "65n", "128n", "-1n", "-3n", "-64n", "-65n", "2n**32n", "2n**64n", "-(2n**64n)", "1000n"];
for (const v of shiftVals) for (const s of shiftBy) {
  if (/^\d+n$/.test(s) && s.length > 10) continue;
  add(T(`(${v})<<(${s})`), T(`(${v})>>(${s})`), T(`(${v})>>>(${s})`));
}
for (const v of ["1n", "-1n", "5n", "2n**64n", "-(2n**64n)-3n", "0n", "-7n"]) for (const w of ["3n", "-3n", "0n", "-1n", "2n**70n", "-(2n**70n)", "1n", "6n", "2n**64n-1n"]) add(T(`[(${v})&(${w}),(${v})|(${w}),(${v})^(${w})].join()`));
add(T("1n<<1000000n>0n"), T("(1n<<64n)-1n"), T("-1n>>1000n"), T("1n>>1000n"), T("1n<<-1n"), T("1n<<31n"), T("1n<<2n**30n"), T("1n<<-(2n**64n)"), T("1<<1n"), T("1n<<1"), T("1n>>>1"), T("2n>>>0n"), T("-1n>>>0n"), T("({valueOf(){return 3n}})<<2n"));

// ---- 8. Divisão, resto e exponente negativo, por zero.
const divVals = ["0n", "1n", "-1n", "7n", "-7n", "10n", "2n**64n", "-(2n**64n)", "2n**64n+1n", "-(2n**64n)-1n"];
for (const a of divVals) for (const b of divVals) add(T(`[(${a})/(${b}),(${a})%(${b})].join()`), T(`(${a})**(${b})`));
for (const a of divVals) add(T(`(${a})/0n`), T(`(${a})%0n`), T(`(${a})/-0n`), T(`(${a})/0`), T(`(${a})%0`), T(`(${a})/Object(0n)`), T(`{var x=${a};x/=0n;return x}`), T(`{var x=${a};x%=0n;return x}`));
add(T("-7n/2n"), T("-7n%2n"), T("7n/-2n"), T("7n%-2n"), T("-7n%-2n"), T("-6n%3n"), T("Object.is(-6n%3n,0n)"), T("(2n**128n)/(2n**64n)"), T("(2n**128n+7n)%(2n**64n)"), T("-(2n**128n)/3n"), T("1n/3n*3n"), T("(10n**40n)/(10n**20n)"), T("0n/5n"), T("0n/-5n"));

// ---- 9. Literais.
const literals = [
  "0n", "1n", "-1n", "0x1fn", "0XFFn", "0b101n", "0B11n", "0o17n", "0O7n", "1_000n", "0x_1n", "1__0n", "1_n", "00n", "01n", "08n", "1.5n", "1e3n", "1.n", ".5n", "0xn", "0b2n", "0o8n", "0b1_0n", "0xFFFFFFFFFFFFFFFFFFFFn",
  "123456789012345678901234567890n", "0n.toString()", "1n.toString()", "1 n", "1N", "0x1N", "1nn", "1n1", "0n1", "1n.constructor.name", "1n++", "typeof 1n", "-0n", "+1n", "(1n)", "[1n,2n]", "{a:1n}", "1_0n", "0_0n", "0.0n", "0e0n", "9007199254740993n", "0xfFn", "0b0n", "0o0n", "1n ** 2n", "-1n ** 2n", "(-1n) ** 2n",
  "1n+-+1n", "3n*-2n", "0x10n.toString(2)", "`${1n}`", "1n in {1:2}", "1n instanceof Object", "01_0n", "1e1_0n", "0_1n", "0b_1n", "1n_",
];
for (const l of literals) add(T(`(0,eval)(${JSON.stringify(l)})`), T(`new Function('return '+${JSON.stringify(l)})()`));
add(T("typeof 0n"), T("0n===0n"), T("0x10n===16n"), T("0b11n+0o7n+0xan"), T("1_000n===1000n"), T("[1n,2n,3n].reduce((a,b)=>a+b)"), T("[1n,2n,3n].reduce((a,b)=>a+b,0)"), T("({1n:1})['1']"), T("({0x10n:1})"), T("class A{static 1n=2}"));

// ---- 10. Parse de string (BigInt, ==, <, asIntN) em grade fina.
const parseStrs = [
  "", " ", "0", "-0", "+0", "00", "007", "-007", "1", "-1", "+1", "10", "0x10", "0X10", "-0x10", "0b10", "0B10", "0o10", "0O10", "0xg", "0x", "0b", "0o", "1e2", "1E2", "1.", ".1", "1.0", "1,0", "1_0", "1n", "n", "Infinity", "-Infinity", "NaN", "null",
  "undefined", "true", " 1", "1 ", " 1 ", "\t1\n", " 1", " 1 ", "﻿1", "​1", "1\u0000", "１", "٣", "9007199254740993", "-9007199254740993", "18446744073709551616", "340282366920938463463374607431768211456",
  "0".repeat(40) + "1", "1" + "0".repeat(60), "0x" + "f".repeat(40), "0b" + "1".repeat(70), "- 1", "+ 1", "++1", "+-1", "-+1", "1-", "1+", "0x-1", "0x+1", "0b-1", "1 1", "1.5", "-1.5", "0.0", "1e0", "0e0",
];
for (const s of parseStrs) {
  const lit = JSON.stringify(s);
  add(T(`BigInt(${lit})`), T(`BigInt.asIntN(8,${lit})`), T(`1n==${lit}`), T(`${lit}==0n`), T(`0n<${lit}`), T(`${lit}<=1n`), T(`BigInt(${lit}+'0')`), T(`BigInt(Object(${lit}))`), T(`BigInt.asUintN(64,${lit})`));
}

// ---- 11. JSON com BigInt.
add(
  T("JSON.stringify(1n)"), T("JSON.stringify([1n])"), T("JSON.stringify({a:1n})"), T("JSON.stringify(Object(1n))"), T("JSON.stringify({a:undefined,b:1n})"), T("JSON.stringify(1n,()=>2)"), T("JSON.stringify({a:1n},(k,v)=>typeof v=='bigint'?String(v):v)"),
  T("JSON.stringify({a:1n},(k,v)=>typeof v=='bigint'?v.toString()+'n':v)"), T("JSON.stringify({a:[1n,{b:2n}]},(k,v)=>typeof v=='bigint'?Number(v):v)"), T("JSON.stringify(1n,null,2)"),
  T("{BigInt.prototype.toJSON=function(){return this.toString()};return JSON.stringify({a:1n,b:[2n]})}"), T("{BigInt.prototype.toJSON=function(){return typeof this};return JSON.stringify(1n)}"),
  T("{BigInt.prototype.toJSON=function(){return 5n};return JSON.stringify(1n)}"), T("{BigInt.prototype.toJSON=function(){return 5};return JSON.stringify([1n,Object(2n)])}"),
  T("{var log=[];JSON.stringify({a:1n},function(k,v){log.push(k+':'+typeof v);return typeof v=='bigint'?0:v});return log.join()}"),
  T("{BigInt.prototype.toJSON=function(k){return k};return JSON.stringify({x:1n,y:[3n]})}"), T("{Object.defineProperty(BigInt.prototype,'toJSON',{get(){return ()=>typeof this}});return JSON.stringify(1n)}"),
  T("JSON.stringify({toJSON(){return 1n}})"), T("JSON.stringify(Symbol())+','+JSON.stringify([Symbol()])+','+JSON.stringify({a:Symbol()})"), T("JSON.stringify({a:1n},null,'  ')"),
  T("JSON.parse('123',(k,v)=>typeof v)"), T("JSON.parse('12345678901234567890')"), T("JSON.parse('12345678901234567890',(k,v,c)=>c&&c.source)"), T("JSON.parse('[1,2]',(k,v)=>typeof v=='number'?BigInt(v):v)+''"),
  T("JSON.parse('1n')"), T("JSON.parse('\"1\"',(k,v)=>BigInt(v))"), T("typeof JSON.rawJSON"), T("JSON.isRawJSON&&JSON.isRawJSON({})"), T("JSON.stringify({a:JSON.rawJSON('12345678901234567890')})"),
  T("JSON.stringify(JSON.rawJSON('1'))"), T("JSON.stringify([1n,2n].map(String))"), T("structuredClone(1n)"), T("structuredClone([1n,Object(2n)]).map(x=>typeof x).join()"), T("structuredClone(Symbol())"),
  T("structuredClone({[Symbol('a')]:1,b:2})"),
);

// ---- 12. Symbol básico.
const symbolInputs = ["", "'a'", "'description'", "''", "undefined", "null", "1", "1n", "true", "{}", "[]", "[1,2]", "{toString(){return 'ts'}}", "{valueOf(){return 'vo'}}", "Symbol.iterator", "NaN", "-0", "'\\u0000'", "'é'", "'\\ud800'", "'a b'", "{[Symbol.toPrimitive](){return 'tp'}}", "()=>1", "new Error('e')"];
for (const i of symbolInputs) add(T(`Symbol(${i})`), T(`Symbol(${i}).description`), T(`Symbol(${i}).toString()`), T(`typeof Symbol(${i})`), T(`new Symbol(${i})`), T(`Symbol.for(${i}).description`), T(`Symbol.keyFor(Symbol.for(${i}))`),
  T(`Symbol.keyFor(Symbol(${i}))`), T(`Symbol.for(${i})===Symbol.for(${i})`), T(`Symbol(${i})===Symbol(${i})`), T(`Object(Symbol(${i})).description`), T(`Object.getOwnPropertyDescriptor(Symbol(${i}).__proto__,'description').get.call(Symbol(${i}))`),
  T(`Symbol(${i}).valueOf()===Symbol(${i}).valueOf()`), T(`String(Symbol(${i}))`), T(`Symbol(${i})[Symbol.toPrimitive]('number')`), T(`Symbol(${i}).toString===Symbol.prototype.toString`));
add(
  T("Symbol.length+Symbol.name"), T("Object.getOwnPropertyNames(Symbol).join()"), T("Object.getOwnPropertyNames(Symbol.prototype).join()"), T("Object.getOwnPropertySymbols(Symbol.prototype).map(String).join()"),
  T("Symbol.prototype[Symbol.toStringTag]"), T("Object.prototype.toString.call(Symbol())"), T("Object.prototype.toString.call(Object(Symbol()))"), T("Symbol.prototype.constructor===Symbol"),
  T("Symbol.for.length+Symbol.keyFor.length"), T("Symbol.keyFor('a')"), T("Symbol.keyFor()"), T("Symbol.keyFor(Object(Symbol.for('x')))"), T("Symbol.keyFor(Symbol.iterator)"), T("Symbol.for('x')===Symbol.for('x')"),
  T("Symbol.for()"), T("Symbol.for().description"), T("Symbol().description"), T("Symbol(undefined).description"), T("Symbol('').description===''"), T("Symbol.for(undefined).toString()"), T("Symbol.for('undefined')===Symbol.for()"),
  T("Symbol.prototype.toString.call('a')"), T("Symbol.prototype.valueOf.call({})"), T("Symbol.prototype.toString.call(Object(Symbol('w')))"), T("Symbol.prototype.valueOf.call(Object(Symbol.iterator))===Symbol.iterator"),
  T("Symbol.prototype.description"), T("Object.getOwnPropertyDescriptor(Symbol.prototype,'description').set"), T("Object.getOwnPropertyDescriptor(Symbol.prototype,Symbol.toPrimitive).writable+','+Object.getOwnPropertyDescriptor(Symbol.prototype,Symbol.toPrimitive).configurable"),
  T("Symbol.prototype[Symbol.toPrimitive].name"), T("Symbol.prototype[Symbol.toPrimitive].length"), T("Symbol.prototype[Symbol.toPrimitive].call(Object(Symbol.iterator),'default')===Symbol.iterator"), T("Symbol.prototype[Symbol.toPrimitive].call(1)"),
  T("Symbol.prototype[Symbol.toPrimitive].call(Symbol('q'),'string')"), T("Symbol.prototype[Symbol.toPrimitive].call(Symbol('q'),'zzz').description"), T("Symbol.prototype[Symbol.toPrimitive].call(Symbol('q')).description"),
  T("Object.isFrozen(Symbol())"), T("Object.isExtensible(Symbol())"), T("{var s=Symbol();s.x=1;return s.x}"), T("(()=>{'use strict';var s=Symbol();s.x=1})()"), T("Symbol().constructor===Symbol"), T("Symbol().hasOwnProperty('description')"),
  T("Object.keys(Object(Symbol())).length"), T("Object.getOwnPropertyNames(Object(Symbol())).length"), T("Object.getPrototypeOf(Symbol())===Symbol.prototype"), T("Symbol() instanceof Symbol"), T("Object(Symbol()) instanceof Symbol"),
  T("class A extends Symbol{};new A"), T("Reflect.construct(Symbol,[],Object)"), T("Symbol.call(1,'x').toString()"), T("Symbol.apply(null,['y']).toString()"), T("[Symbol('a'),Symbol('b')].map(String).join()"),
  T("Symbol('a')==Symbol('a')"), T("{var s=Symbol();return s==s}"), T("{var s=Symbol();return s===Object(s)}"), T("{var s=Symbol();return s==Object(s)}"), T("{var s=Symbol();return Object(s)==Object(s)}"), T("{var s=Symbol();return Object(s)==s}"),
  T("typeof Object(Symbol())"), T("Object(Symbol.iterator)==Symbol.iterator"), T("new Set([Symbol.for('a'),Symbol.for('a'),Symbol('a')]).size"), T("new Map([[Symbol.iterator,1]]).get(Symbol.iterator)"),
  T("new WeakMap().set(Symbol('x'),1)"), T("new WeakSet().add(Symbol.for('x'))"), T("new WeakSet().add(Symbol.iterator)"), T("new WeakRef(Symbol('q')).deref().toString()"), T("new WeakRef(Symbol.for('q'))"),
  T("Symbol('x').description.length"), T("Symbol.for('a b').description"), T("Symbol('\\n').toString()"), T("[Symbol.iterator].includes(Symbol.iterator)"), T("[Symbol.for('z')].indexOf(Symbol.for('z'))"), T("Object.is(Symbol.for('z'),Symbol.for('z'))"),
);

// ---- 13. Well-knowns.
const wellKnown = ["iterator", "asyncIterator", "hasInstance", "isConcatSpreadable", "match", "matchAll", "replace", "search", "species", "split", "toPrimitive", "toStringTag", "unscopables", "dispose", "asyncDispose", "observable", "metadata", "description", "length", "foo"];
for (const n of wellKnown) {
  const s = `Symbol.${n}`;
  add(T(`typeof ${s}`), T(`String(${s})`), T(`${s}.description`), T(`Object.getOwnPropertyDescriptor(Symbol,'${n}')`.replace(/^T\(\(\)=>/, "")), T(`D1(Symbol,'${n}')`.replace("D1", "((o,k)=>{var d=Object.getOwnPropertyDescriptor(o,k);return d?[typeof d.value,d.writable,d.enumerable,d.configurable].join():'none'})")),
    T(`Symbol.keyFor(${s})`), T(`${s}===Symbol.for('Symbol.${n}')`), T(`Symbol.for('Symbol.${n}')===${s}`), T(`Symbol('Symbol.${n}')===${s}`), T(`Object.getOwnPropertySymbols({[${s}]:1}).length`),
    T(`({[${s}]:1})[${s}]`), T(`${s}.toString()`), T(`Object(${s}).description`), T(`${s}+''`), T(`[${s}].map(String)[0]`), T(`Object.getOwnPropertyNames(Symbol).indexOf('${n}')>=0`));
}
add(
  T("Object.getOwnPropertyNames(Symbol).filter(k=>typeof Symbol[k]=='symbol').sort().join()"), T("Object.getOwnPropertyNames(Symbol).join()"), T("Reflect.ownKeys(Symbol).length"),
  T("Object.getOwnPropertyNames(Symbol).filter(k=>typeof Symbol[k]=='symbol').map(k=>Symbol[k].description===('Symbol.'+k)).join()"),
  T("Symbol.iterator in []"), T("[][Symbol.iterator]===[][Symbol.iterator]"), T("typeof ''[Symbol.iterator]"), T("typeof (new Map)[Symbol.iterator]"), T("typeof (function*(){})()[Symbol.iterator]"),
  T("[...'ab'].join()"), T("Array.from({length:2,[Symbol.iterator]:function*(){yield 9}})+''"), T("[].concat({length:1,0:'x',[Symbol.isConcatSpreadable]:true}).length"), T("[].concat(Object.assign([1,2],{[Symbol.isConcatSpreadable]:false})).length"),
  T("{class A{static [Symbol.hasInstance](x){return x===1}};return 1 instanceof A}"), T("{var o={[Symbol.toStringTag]:'Zed'};return String(o)}"), T("'abc'.replace({[Symbol.replace](s,r){return s+r}},'!')"),
  T("'abc'.split({[Symbol.split](s,l){return [s,l]}},3)+''"), T("'abc'.search({[Symbol.search](s){return 42}})"), T("'abc'.match({[Symbol.match](s){return s+'m'}})"), T("[...'abc'.matchAll({[Symbol.matchAll](s){return [s,1]}})]+''"),
  T("Object.keys(Array.prototype[Symbol.unscopables]).sort().join()"), T("Object.getPrototypeOf(Array.prototype[Symbol.unscopables])"), T("Object.getOwnPropertyNames(Symbol.prototype)+''"),
  T("{var r=/a/;r[Symbol.match]=false;return '/a/'.startsWith(r)}"), T("'/a/'.startsWith(/a/)"), T("RegExp[Symbol.species]===RegExp"), T("Array[Symbol.species]===Array"), T("Object.getOwnPropertyDescriptor(Array,Symbol.species).get.name"),
  T("Date.prototype[Symbol.toPrimitive].call(new Date(0),'number')"), T("Date.prototype[Symbol.toPrimitive].call(new Date(0),'x')"), T("Date.prototype[Symbol.toPrimitive].call({},'number')"), T("Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3}},'number')"),
  T("Date.prototype[Symbol.toPrimitive].call({toString(){return 'ts'}},'string')"), T("Date.prototype[Symbol.toPrimitive].call({toString(){return 'ts'}},'default')"), T("Date.prototype[Symbol.toPrimitive].name+Date.prototype[Symbol.toPrimitive].length"),
  T("Function.prototype[Symbol.hasInstance].call(Array,[])"), T("Function.prototype[Symbol.hasInstance].name"), T("Object.getOwnPropertyDescriptor(Function.prototype,Symbol.hasInstance).writable"),
);

// ---- 14. Conversões implícitas de símbolo que lançam (grade de usos).
const symUses = [
  "''+$", "$+''", "`${$}`", "+$", "-$", "~$", "$*1", "1*$", "$+1", "1+$", "$-1", "$|0", "$<1", "1<$", "$<=$", "$==1", "$=='x'", "$==$", "$===$", "!$", "!!$", "$&&1", "$||1", "$??1", "typeof $", "void $", "String($)", "Number($)", "Boolean($)",
  "BigInt($)", "parseInt($)", "parseFloat($)", "isNaN($)", "Math.abs($)", "Math.max($)", "Math.round($)", "$.toString()", "$.valueOf()===$", "$.description", "String($.valueOf())", "[$]+''", "[$].join()", "[$].toString()", "[$].map(String)+''", "`${[$]}`", "JSON.stringify($)", "JSON.stringify([$])",
  "JSON.stringify({a:$})", "JSON.stringify({[$]:1})", "new Array($)", "Array($).length", "[1,2][$]", "({a:1})[$]", "({[$]:1})[$]", "$ in {}", "$ in {[$]:1}", "Object.keys({[$]:1}).length", "'abc'.indexOf($)", "'abc'.concat($)", "'abc'.includes($)", "'abc'.split($)",
  "'abc'.replace($,'x')", "'abc'.padStart(5,$)", "'abc'.repeat($)", "'abc'.at($)", "new RegExp($)", "RegExp($)", "String.raw`${$}`", "'abc'.localeCompare($)", "[1,2,3].join($)", "[1,2,3].at($)", "[$,$].indexOf($)", "[$].includes($)", "new Set([$]).has($)",
  "Object($)+''", "Object($)+1", "Object($)==$", "Object($)===$", "Object($).valueOf()===$", "Object($)<1", "Number(Object($))", "String(Object($))", "[Object($)]+''", "Symbol($)", "Symbol.for($)", "Symbol.keyFor($)", "Object.assign({}, $)", "Object.assign({},{[$]:1})[$]",
  "Object.entries($)", "Object.keys($)", "Object.getPrototypeOf($)===Symbol.prototype", "new Date($)", "new Number($)", "new String($)", "Number.isNaN($)", "Number.parseFloat($)", "Object.is($,$)", "Array.from($)", "[...$]", "Array.of($)[0]===$", "(()=>{var x=$;x++})()", "(()=>{var x=$;x+='a'})()",
  "(()=>{var x='a';x+=$})()", "(()=>{var x=1;x*=$})()", "(()=>{var x=$;x||=1;return x===$})()", "isFinite($)", "encodeURIComponent($)", "escape($)", "atob($)", "console.log===0&&$", "$.constructor===Symbol", "$.length", "$[0]", "$.foo", "$.foo=1", "delete $.x", "$?.x", "$.call", "new $", "$()", "`${{toString(){return $}}}`",
  "({toString(){return $}})+''", "({valueOf(){return $}})+1", "({valueOf(){return $}})==1", "({[Symbol.toPrimitive](){return $}})+''", "({[Symbol.toPrimitive](){return $}})==$", "({[Symbol.toPrimitive](){return $}})<1", "({[Symbol.toPrimitive](){return $}})[$]", "[1,2,3].sort(()=>$)", "Math.max({valueOf(){return $}})",
  "Reflect.ownKeys({[$]:1}).length", "Reflect.has({[$]:1},$)", "Reflect.get({[$]:5},$)", "Reflect.defineProperty({},$,{value:1})", "Object.defineProperty({},$,{value:1,enumerable:true})[$]", "Object.getOwnPropertyDescriptor({[$]:1},$).value", "Object.hasOwn({[$]:1},$)", "({[$]:1}).hasOwnProperty($)", "({[$]:1}).propertyIsEnumerable($)",
  "String(Symbol.prototype.toString.call($))", "Symbol.prototype.toString.call($)==='Symbol()'", "Function.prototype.call.call($)", "$.toLocaleString()", "$.toString(2)", "[$].toLocaleString()", "[$].toString===Array.prototype.toString", "Intl.NumberFormat().format($)", "'a'.localeCompare($,undefined,{})",
];
const symVals = ["Symbol()", "Symbol('d')", "Symbol.iterator", "Symbol.for('k')"];
for (const u of symUses) for (const sv of symVals.slice(0, u.includes("{[$]") || u.includes("$.") ? 4 : 2)) add(T(u.split("$").join(sv)));

// ---- 15. Symbol como chave de propriedade: ownKeys, assign, spread, enumeração.
add(
  T("{var a=Symbol('a'),b=Symbol('b');var o={x:1,[a]:2,y:3,[b]:4,1:5};return Reflect.ownKeys(o).map(String).join()}"),
  T("{var a=Symbol('a');var o={[a]:1,b:2};return Object.keys(o)+'|'+Object.getOwnPropertyNames(o)+'|'+Object.getOwnPropertySymbols(o).length+'|'+JSON.stringify(o)}"),
  T("{var a=Symbol('a');var o={[a]:1,b:2};var r=[];for(var k in o)r.push(k);return r.join()}"), T("{var a=Symbol('a');var o={[a]:1,b:2};return Object.entries(o).join()+'|'+Object.values(o)}"),
  T("{var a=Symbol('a');var o=Object.assign({}, {[a]:1,b:2});return Reflect.ownKeys(o).map(String).join()}"), T("{var a=Symbol('a');var o={...{[a]:1,b:2}};return Reflect.ownKeys(o).map(String).join()+o[a]}"),
  T("{var a=Symbol('a');var s={};Object.defineProperty(s,a,{value:1,enumerable:false});return Reflect.ownKeys(Object.assign({},s)).length+','+Reflect.ownKeys({...s}).length+','+Reflect.ownKeys(s).length}"),
  T("{var a=Symbol('a');var s={};Object.defineProperty(s,a,{get(){return 7},enumerable:true});var t=Object.assign({},s);return Object.getOwnPropertyDescriptor(t,a).value}"),
  T("{var a=Symbol('a');var o={[a]:1};return JSON.stringify(Object.getOwnPropertyDescriptors(o)[a])}"), T("{var a=Symbol('a');var o={[a]:1};return Object.getOwnPropertyDescriptors(o)[a].enumerable}"),
  T("{var a=Symbol('a');var o={[a]:1};delete o[a];return Reflect.ownKeys(o).length}"), T("{var a=Symbol('a');var o={};o[a]=1;o[a]++;return o[a]}"), T("{var a=Symbol('a');var o={[a]:1};return a in o}"), T("{var a=Symbol('a');var o={[a]:1};return Object.hasOwn(o,a)+','+o.hasOwnProperty(a)}"),
  T("{var a=Symbol('a');var o=Object.create({[a]:1});return o[a]+','+(a in o)+','+Object.hasOwn(o,a)}"), T("{var a=Symbol('a'),b=Symbol('a');var o={[a]:1,[b]:2};return o[a]+','+o[b]+','+Reflect.ownKeys(o).length}"),
  T("{var o={[Symbol.for('k')]:1};return o[Symbol.for('k')]}"), T("{var o={[Symbol('k')]:1};return o[Symbol('k')]}"), T("{var a=Symbol('a');var o=Object.freeze({[a]:1});o[a]=2;return o[a]}"), T("{var a=Symbol('a');var o=Object.freeze({[a]:1});return (()=>{'use strict';o[a]=2})()}"),
  T("{var a=Symbol('a');var o=Object.seal({[a]:1});return Object.isSealed(o)+','+Object.isFrozen(o)}"), T("{var a=Symbol('a');var o=Object.preventExtensions({});o[a]=1;return Reflect.ownKeys(o).length}"), T("{var a=Symbol('a');var o=Object.preventExtensions({});return Reflect.set(o,a,1)}"),
  T("{var a=Symbol('a');return Object.fromEntries([[a,1],['b',2]])[a]}"), T("{var a=Symbol('a');return Reflect.ownKeys(Object.fromEntries([[a,1],['b',2]])).map(String).join()}"), T("{var a=Symbol('a');return Object.entries({[a]:1}).length}"),
  T("{var a=Symbol('a');var m=new Map([[a,1]]);return [...m.keys()].map(String)+''}"), T("{var a=Symbol('a');return Object.groupBy([1,2,3],x=>x%2?a:'e')[a]+''}"), T("{var a=Symbol('a');return Reflect.ownKeys(Object.groupBy([1],()=>a)).map(String)+''}"),
  T("{var a=Symbol('a');class C{[a](){return 1}static [a]=2;get [Symbol.toStringTag](){return 'CC'}};return new C()[a]()+','+C[a]+','+String(new C)}"), T("{var a=Symbol('desc');var f={[a](){}}[a];return f.name}"), T("{var a=Symbol();var f={[a](){}}[a];return JSON.stringify(f.name)}"),
  T("{var a=Symbol('desc');var f={[a]:function(){}}[a];return f.name}"), T("{var a=Symbol('desc');var f={get [a](){return 1}};return Object.getOwnPropertyDescriptor(f,a).get.name}"), T("{var a=Symbol('desc');class C{static [a](){}};return C[a].name}"),
  T("{var a=Symbol('x');var o={[a]:1};return Object.getOwnPropertyNames(o).length+Object.getOwnPropertySymbols(o).length}"), T("{var o={};o[Symbol.iterator]=1;o[Symbol.toStringTag]='W';return Reflect.ownKeys(o).map(String)+'|'+String(o)}"),
  T("{var a=Symbol('a');var o=Object.defineProperties({}, {[a]:{value:1,enumerable:true},b:{value:2}});return Reflect.ownKeys(o).map(String)+''}"), T("{var a=Symbol('a');var o={[a]:1};return Object.keys(Object.create(o)).length+','+Reflect.ownKeys(Object.create(o)).length}"),
  T("{var a=Symbol('a');var o={[a]:1,2:1,b:1,1:1,[Symbol('z')]:1,a:1};return Reflect.ownKeys(o).map(String).join()}"), T("{var a=Symbol('a');var p=new Proxy({},{ownKeys(){return [a,'x']},getOwnPropertyDescriptor(t,k){return {value:1,enumerable:true,configurable:true}}});return Reflect.ownKeys(p).map(String)+'|'+Object.keys(p)+'|'+Object.getOwnPropertySymbols(p).length}"),
  T("{var a=Symbol('a');var p=new Proxy({},{ownKeys(){return [a,'x']},getOwnPropertyDescriptor(t,k){return {value:1,enumerable:true,configurable:true}}});return Reflect.ownKeys({...p}).map(String)+''}"), T("{var p=new Proxy({},{ownKeys(){return [1]}});return Reflect.ownKeys(p)}"),
  T("{var log=[];var p=new Proxy({},{get(t,k){log.push(typeof k+':'+String(k));return 1}});p[Symbol.iterator];p.a;`${Object.keys({...p})}`;return log.join()}"), T("{var log=[];var p=new Proxy({},{get(t,k){log.push(String(k));return undefined}});String(p);p+'';return log.join()}"),
  T("{var log=[];var p=new Proxy({},{get(t,k){log.push(String(k));return undefined}});try{p+1}catch(e){};return log.join()}"), T("{var log=[];var p=new Proxy([],{get(t,k){log.push(String(k));return t[k]}});[].concat(p);return log.join()}"),
  T("{var log=[];var p=new Proxy({},{has(t,k){log.push(String(k));return false}});Symbol.iterator in p;with(p){}return log.join()}"), T("{var a=Symbol('a');var o={a:1,[a]:2};return Object.entries(Object.getOwnPropertyDescriptors(o)).length+','+Reflect.ownKeys(Object.getOwnPropertyDescriptors(o)).length}"),
  T("{var a=Symbol('a');var o=[1,2];o[a]=3;return o.length+','+Object.keys(o)+','+Reflect.ownKeys(o).map(String)}"), T("{var a=Symbol('a');var o=[1,2];o[a]=3;return JSON.stringify(o)+','+o.concat([4]).length+','+Reflect.ownKeys(o.slice()).length}"),
  T("{var a=Symbol('a');var o=function(){};o[a]=1;return Reflect.ownKeys(o).map(String).join()}"), T("{var a=Symbol('a');var o=new Uint8Array(1);o[a]=1;return Reflect.ownKeys(o).map(String).join()}"), T("{var a=Symbol('a');return Object.getOwnPropertySymbols(Object.assign(()=>{},{[a]:1})).length}"),
  T("{var a=Symbol('a');return Reflect.ownKeys(structuredClone({[a]:1,b:2})).map(String)+''}"), T("{var a=Symbol('a');var o={[a]:1};return Object.entries(o).length+JSON.stringify(Object.entries(o))}"), T("{var a=Symbol('a');return Object.getOwnPropertyNames(String(a)).length}"),
  T("{var a=Symbol('a');var o={};Object.defineProperty(o,a,{value:1});return Object.isFrozen(Object.freeze(o))+','+Object.getOwnPropertyDescriptor(o,a).writable}"), T("{var a=Symbol('a');var o={[a]:1};return Object.keys(o).concat(Object.getOwnPropertySymbols(o)).length}"),
  T("{var a=Symbol('a');var o={};o[a]=1;o[Object(a)]=2;return Reflect.ownKeys(o).length+','+o[a]}"), T("{var a=Symbol('a');var k=Object(a);var o={[k]:1};return Reflect.ownKeys(o)[0]===a}"), T("{var a=Symbol('a');var o={[a]:1};return o[Object(a)]}"),
);

// ---- 16. Grade de conversões abstratas: valores × operadores, com log da ordem de valueOf/toString/toPrimitive.
const convVals = [
  "0", "-0", "1", "-1.5", "NaN", "Infinity", "''", "'1'", "' 12 '", "'abc'", "'0x10'", "'1e3'", "true", "null", "undefined", "1n", "0n", "-5n", "[]", "[1]", "[1,2]", "{}", "Symbol('s')", "Symbol.iterator",
  "Q('a',1,'s')", "Q('a','5',1)", "Q('a',{},'t')", "Q('a',{},{})", "Q('a',null,'x')", "Q('a',undefined,1)", "Q('a',3n,'z')", "Q('a',Symbol('q'),'z')", "Q('a',true,'z')", "Q('a',-0,'z')",
  "TP('a',1)", "TP('a','str')", "TP('a',2n)", "TP('a',{})", "TP('a',Symbol('p'))", "TP('a',null)", "TP('a',undefined)", "{[Symbol.toPrimitive]:undefined,valueOf(){L.push('fv');return 8},toString(){L.push('fs');return 'f'}}",
  "{[Symbol.toPrimitive]:null,valueOf(){L.push('nv');return 8}}", "{[Symbol.toPrimitive]:1}", "{[Symbol.toPrimitive]:{}}", "{valueOf:1,toString(){L.push('s');return 'ok'}}", "{valueOf(){L.push('v');return {}},toString:null}", "{valueOf:undefined,toString:undefined}",
  "Object(1n)", "Object(Symbol.iterator)", "Object('str')", "Object(7)", "Object(true)", "Object(null)", "new Date(0)", "[Q('i',1,'s')]", "[TP('i',9)]", "{get [Symbol.toPrimitive](){L.push('get');return ()=>4}}", "{get valueOf(){L.push('gv');return ()=>4}}",
];
const convOps = [
  "Number($)", "String($)", "+($)", "''+($)", "`${$}`", "($)+1", "($)*1", "($)-1", "($)-1n", "1n+($)", "($)==0", "($)=='1'", "($)==1n", "($)<2", "2>($)", "($)<=($)", "($)==($)", "($)==null", "isNaN($)", "parseInt($)", "parseFloat($)",
  "Math.max($,0)", "BigInt($)", "!!($)", "typeof($)", "JSON.stringify($)", "[$]+''", "({[$]:1})", "({a:1,undefined:2,null:3,1:4})[$]", "[10,20,30][$]", "Object.keys({[$]:1})+''", "'abc'.indexOf($)", "'abcd'.slice($)", "'ab'.repeat($)", "new Array($).length",
  "($)**2", "($)|0", "~($)", "-($)", "Number.isInteger($)", "'x'.concat($)", "[3,4].includes($)", "Reflect.has({},$)", "$ in {a:1}", "String.prototype.padEnd.call('a',3,$)", "Object.defineProperty({},$,{value:1})", "BigInt.asIntN(8,$)", "BigInt.asUintN($,5n)",
  "[1,2,3].at($)", "Date.prototype.getTime.call(new Date($))", "'a'.localeCompare($)", "Math.pow($,2)", "($)>>>0", "($)<<1", "($)&1n", "Atomics.add(new Int32Array(1),0,$)", "new Uint8Array([$])[0]", "new Float64Array([$])[0]", "String(Object($))", "Object($)==($)", "(x=>x)($)+''",
];
for (const v of convVals) for (const op of convOps) {
  const body = op.split("$").join(v);
  add(`U(()=>${body})`);
}
// Ordem de avaliação e de conversão em operandos duplos.
const pairOps = ["+", "-", "*", "**", "<", ">", "<=", ">=", "==", "!=", "&", "<<", "%"];
const pairVals = [
  ["Q('l',1,'s')", "Q('r',2,'t')"], ["Q('l','a',1)", "Q('r',2,1)"], ["Q('l',1n,1)", "Q('r',2n,1)"], ["Q('l',1n,1)", "Q('r',2,1)"], ["TP('l',1)", "TP('r',2)"], ["TP('l','a')", "TP('r','b')"], ["TP('l',1n)", "TP('r','x')"], ["Q('l',{},'s')", "Q('r',{},'t')"],
  ["Q('l',Symbol(),'s')", "Q('r',1,'t')"], ["Q('l',1,'s')", "Symbol()"], ["Symbol()", "Q('r',1,'t')"], ["Q('l',1,'s')", "undefined"], ["null", "Q('r',2,'t')"], ["Q('l',1,'s')", "1n"], ["1n", "Q('r',1,'s')"], ["TP('l',1)", "Symbol()"], ["Symbol()", "TP('r',1)"],
];
for (const [a, b] of pairVals) for (const op of pairOps) add(`U(()=>(${a})${op}(${b}))`);
add(
  "U(()=>[Q('a',1,'x'),Q('b',2,'y')]+'')", "U(()=>String([Q('a',1,'x')]))", "U(()=>`${Q('a',1,'x')}${Q('b',2,'y')}`)", "U(()=>Q('a',1,'x')+Q('b',2,'y')+Q('c',3,'z'))", "U(()=>({[Q('a',1,'x')]:Q('b',2,'y')}))",
  "U(()=>({[Q('k',1,'x')]:Q('v',2,'y')})[Q('k',1,'x')])", "U(()=>{var o={};o[Q('a',1,'x')]=Q('b',2,'y');return o})", "U(()=>{var o={};o[Q('a',1,'x')]+=Q('b',2,'y');return o})", "U(()=>{var o={};o[Q('a',1,'x')]++;return o})",
  "U(()=>{var o={a:1};delete o[Q('a','a',1)];return o})", "U(()=>Q('a','a',1) in {a:1})", "U(()=>{var o={a:1};return o?.[Q('a','a',1)]})", "U(()=>{var o=null;return o?.[Q('a','a',1)]})", "U(()=>{var o=null;return o[Q('a','a',1)]})", "U(()=>{var o=undefined;o[Q('a','a',1)]=Q('b',1,1)})",
  "U(()=>{var o=null;o[Q('a','a',1)]=Q('b',1,1)})", "U(()=>null[Q('a','a',1)])", "U(()=>(void 0)[TP('a','p')])", "U(()=>{var o={};o[TP('a','p')]=1;return Object.keys(o)+''})", "U(()=>{var o={};o[TP('a',Symbol('q'))]=1;return Reflect.ownKeys(o).map(String)+''})",
  "U(()=>{var o={};o[TP('a',{})]=1;return Reflect.ownKeys(o)})", "U(()=>[1,2,3][TP('a',1)])", "U(()=>[1,2,3][TP('a',1n)])", "U(()=>[1,2,3][Q('a',1,'x')])", "U(()=>[1,2,3][Q('a','1',1)])", "U(()=>[1,2,3][Q('a',{},'2')])", "U(()=>({'[object Object]':1})[{}])", "U(()=>({'1,2':1})[[1,2]])",
  "U(()=>({'a':1})[Q('a',{},'a')])", "U(()=>String(Q('a',{},{})))", "U(()=>Number(Q('a',{},{})))", "U(()=>`${Q('a',{},{})}`)", "U(()=>Q('a',{},{})+1)", "U(()=>Q('a',{},{})==1)", "U(()=>Q('a',{},{})==Q('a',{},{}))", "U(()=>{var q=Q('a',1,1);return q==q})",
  "U(()=>{var q=Q('a',1,1);return q===q})", "U(()=>Q('a',1,1)<Q('b',1,1))", "U(()=>Q('a',1,1)>Q('b',1,1))", "U(()=>Q('a',1,1)<=Q('b',1,1))", "U(()=>Q('a',1,1)>=Q('b',1,1))", "U(()=>'1'<Q('a',1,1))", "U(()=>Q('a',1,1)<'1')", "U(()=>Q('a','x',1)<Q('b','y',1))", "U(()=>Q('a','10',1)<Q('b','9',1))",
  "U(()=>Q('a','10',1)<9)", "U(()=>Q('a',10,1)<Q('b','9',1))", "U(()=>Q('a',1n,1)<Q('b','2',1))", "U(()=>Q('a',1n,1)<Q('b','x',1))", "U(()=>Math.max(Q('a',1,'x'),Q('b',2,'y'),Q('c',NaN,'z')))", "U(()=>Math.min(Q('a',3,1),Q('b',Symbol(),1),Q('c',1,1)))", "U(()=>Math.max(Q('a',Symbol(),1),Q('b',1,1)))",
  "U(()=>Math.hypot(Q('a',3,1),Q('b',4,1)))", "U(()=>Math.atan2(Q('a',1,1),Q('b',1,1)))", "U(()=>Math.pow(Q('a',2,1),Q('b',3,1)))", "U(()=>Math.imul(Q('a',2,1),Q('b',3,1)))", "U(()=>[Q('a',3,1),Q('b',1,1)].sort())", "U(()=>[Q('a',3,'3'),Q('b',1,'1'),Q('c',2,'2')].sort()+'')",
  "U(()=>[Q('a',3,'3'),Q('b',1,'1'),Q('c',2,'2')].sort((x,y)=>x-y).length)", "U(()=>['b',Q('a',1,'a')].sort())", "U(()=>[Q('a',1,'x')].join(Q('b',1,'-')))", "U(()=>[1,2].join(Q('b',1,'-')))", "U(()=>[1,2].join(TP('b','+')))", "U(()=>'abc'.padStart(Q('n',6,1),Q('p',1,'xy')))",
  "U(()=>'abc'.slice(Q('a',1,1),Q('b',2,1)))", "U(()=>'abc'.substring(Q('a',2,1),Q('b',0,1)))", "U(()=>'abc'.substr(Q('a',1,1),Q('b',1,1)))", "U(()=>'abc'.indexOf(Q('a','b',1),Q('c',0,1)))", "U(()=>'abc'.replace(Q('a','b',1),Q('c','X',1)))", "U(()=>'abc'.split(Q('a','b',1),Q('c',5,1)))",
  "U(()=>'abc'.startsWith(Q('a','a',1),Q('b',0,1)))", "U(()=>'a-b'.at(Q('a',1,1)))", "U(()=>'abc'.charAt(Q('a',1,1)))", "U(()=>'abc'.charCodeAt(TP('a',1)))", "U(()=>String.fromCharCode(Q('a',65,1),Q('b',66,1)))", "U(()=>String.fromCodePoint(Q('a',65,1),Q('b',-1,1)))",
  "U(()=>'x'.repeat(Q('a',2,1)))", "U(()=>'x'.repeat(Q('a',-1,1)))", "U(()=>'abc'.normalize(Q('a','NFC',1)))", "U(()=>'abc'.localeCompare(Q('a','b',1)))", "U(()=>new String(Q('a','x',1)).length)", "U(()=>new Number(Q('a','5',1))+1)", "U(()=>new Number(TP('a',2n))+1)",
  "U(()=>Number(TP('a',2n)))", "U(()=>Number(Q('a',2n,1)))", "U(()=>BigInt(TP('a',2)))", "U(()=>BigInt(TP('a','2')))", "U(()=>BigInt(Q('a',1.5,'3')))", "U(()=>BigInt(Q('a','x','3')))", "U(()=>BigInt(Q('a',{},'3')))", "U(()=>BigInt.asIntN(Q('b',8,1),Q('v',300n,1)))", "U(()=>BigInt.asIntN(Q('b',8,1),Q('v',300,1)))",
  "U(()=>BigInt.asUintN(TP('b',4),TP('v','255')))", "U(()=>BigInt.asUintN(TP('b',4),TP('v',255)))", "U(()=>(255n).toString(Q('r',16,1)))", "U(()=>(255n).toString(TP('r','2')))", "U(()=>(255).toString(Q('r',16,1)))", "U(()=>(255).toString(Q('r',99,1)))", "U(()=>(1.5).toFixed(Q('d',2,1)))",
  "U(()=>parseInt(Q('s','11',1),Q('r',2,1)))", "U(()=>parseInt(Q('s',{},'11'),Q('r',{},2)))", "U(()=>parseInt(TP('s','0x1f'),TP('r',0)))", "U(()=>parseFloat(Q('s','1.5x',1)))", "U(()=>isNaN(Q('s','x',1)))", "U(()=>Number.isNaN(Q('s','x',1)))", "U(()=>isFinite(Q('s',1,'x')))",
  "U(()=>new Date(Q('a',0,1)).getTime())", "U(()=>new Date(TP('a','1970-01-01T00:00:00Z')).getTime())", "U(()=>new Date(Q('a',1,1),Q('b',0,1)).getFullYear())", "U(()=>Date.UTC(Q('y',1970,1),Q('m',0,1),Q('d',1,1)))", "U(()=>new Date(0)[Symbol.toPrimitive]('default').length>0)",
  "U(()=>new Date(0)-new Date(5))", "U(()=>new Date(5)+1===new Date(5).toString()+'1')", "U(()=>new Date(5)<new Date(6))", "U(()=>new Date(5)==5)", "U(()=>new Date(5)==new Date(5).toString())", "U(()=>+new Date(5))", "U(()=>new Date(5)*1)", "U(()=>`${new Date(NaN)}`)",
  "U(()=>Object.assign({},Q('a',1,'x')))", "U(()=>Object.assign(Q('a',1,'x'),{y:1}).y)", "U(()=>Object(Q('a',1,'x'))===0)", "U(()=>Array.from(Q('a',1,'x')))", "U(()=>Array.from({length:Q('a',2,1)}).length)", "U(()=>Array.from({length:TP('a','2')}).length)", "U(()=>Array.from({length:Q('a',-1,1)}).length)",
  "U(()=>new Array(Q('a',2,1)).length)", "U(()=>new Array(TP('a',2)).length)", "U(()=>new Array(TP('a','2')).length)", "U(()=>Array(TP('a',2n)))", "U(()=>[].concat(TP('a',1)).length)", "U(()=>[1,2,3].slice(Q('a',1,1),Q('b',2,1)))", "U(()=>[1,2,3].splice(Q('a',1,1),Q('b',1,1)))",
  "U(()=>[1,2,3].fill(0,Q('a',1,1),Q('b',2,1)))", "U(()=>[1,2,3].indexOf(2,Q('a',1,1)))", "U(()=>[1,2,3].lastIndexOf(2,Q('a',-1,1)))", "U(()=>[1,2,3].copyWithin(Q('a',0,1),Q('b',1,1)))", "U(()=>[1,2,3].with(Q('a',1,1),9))", "U(()=>[1,2,3].at(Q('a','1',1)))",
  "U(()=>{var a=[];a.length=Q('a',2,1);return a.length})", "U(()=>{var a=[];a.length=TP('a','2');return a.length})", "U(()=>{var a=[];a.length=Q('a',{},{});return a.length})", "U(()=>{var a=[];a.length=2n;return a.length})", "U(()=>{var a=[];a.length='x';return a.length})", "U(()=>{var a=[];a.length=Symbol();return a.length})",
  "U(()=>{var a=[];a.length=-1;return a.length})", "U(()=>{var a=[];a.length=1.5;return a.length})", "U(()=>{var a=[];a.length=4294967296;return a.length})", "U(()=>{var a=[];a.length=4294967295;return a.length})", "U(()=>{var a=[];a.length=Q('a','3',1);return a.length})",
  "U(()=>new Uint8Array(Q('a',2,1)).length)", "U(()=>new Uint8Array(TP('a',2)).length)", "U(()=>new Uint8Array([Q('a',300,1),Q('b','7',1)]).join())", "U(()=>new Float32Array([TP('a',1.1)])[0])", "U(()=>new Uint8Array([TP('a',2n)]))", "U(()=>new BigInt64Array([TP('a',2n)])[0])",
  "U(()=>new BigInt64Array([TP('a',2)]))", "U(()=>new BigInt64Array([Q('a',{},'7')])[0])", "U(()=>new BigInt64Array([Q('a','7',1)])[0])", "U(()=>new BigInt64Array([true])[0])", "U(()=>new BigInt64Array(['x']))", "U(()=>new BigInt64Array([Symbol()]))",
  "U(()=>new Uint8Array(2).fill(Q('a',5,1)).join())", "U(()=>new Uint8Array(2).fill(TP('a',5n)).join())", "U(()=>new BigInt64Array(2).fill(TP('a',5n)).join())", "U(()=>{var t=new Uint8Array(2);t[0]=Q('a',5,1);t[1]=Q('b','7',1);return t.join()})", "U(()=>{var t=new Uint8Array(2);t[5]=Q('a',5,1);return t[5]})",
  "U(()=>{var t=new Uint8Array(2);t[0]=TP('a',9n)})", "U(()=>{var t=new BigInt64Array(2);t[5]=TP('a',9n);return t[5]})", "U(()=>{var t=new BigInt64Array(2);t[5]=TP('a',9)})", "U(()=>{var t=new BigInt64Array(2);t[0]=Q('a',9,1)})", "U(()=>{var t=new BigInt64Array(2);t.fill(Q('a',9,1))})",
  "U(()=>{var t=new Uint8Array(2);t.set([Q('a',1,1),Q('b',2,1)]);return t.join()})", "U(()=>Reflect.ownKeys(Object(Q('a',1,1))).length)", "U(()=>Object.defineProperty({},Q('a','k',1),{value:Q('b',1,1)}))", "U(()=>Object.defineProperty({},TP('a','k'),{value:1,get:Q('b',1,1)}))",
  "U(()=>Object.defineProperty({},'x',{get:Q('a',1,1)}))", "U(()=>Object.defineProperty({},'x',{value:1,enumerable:Q('a',0,1),writable:Q('b',1,1),configurable:TP('c',0)}).x)", "U(()=>Object.getOwnPropertyDescriptor({k:1},Q('a','k',1)).value)", "U(()=>Object.getOwnPropertyDescriptor({k:1},TP('a',Symbol('z'))))",
  "U(()=>Object.hasOwn({k:1},Q('a','k',1)))", "U(()=>Object.hasOwn(Q('a',1,1),Q('b','k',1)))", "U(()=>Object.hasOwn(null,Q('b','k',1)))", "U(()=>({}).hasOwnProperty.call(null,Q('b','k',1)))", "U(()=>({}).hasOwnProperty.call(undefined,Q('b','k',1)))",
  "U(()=>({}).propertyIsEnumerable.call(null,TP('b','k')))", "U(()=>Reflect.get({k:1},Q('a','k',1)))", "U(()=>Reflect.set({},Q('a','k',1),Q('b',2,1)))", "U(()=>Reflect.has({k:1},Q('a','k',1)))", "U(()=>Reflect.deleteProperty({k:1},Q('a','k',1)))", "U(()=>Reflect.defineProperty({},Q('a','k',1),{}))",
  "U(()=>Reflect.getOwnPropertyDescriptor({k:1},TP('a','k')).value)", "U(()=>Reflect.apply(Math.max,null,{length:Q('a',2,1),0:Q('b',1,1),1:Q('c',5,1)}))", "U(()=>Reflect.apply(Math.max,null,{length:TP('a',2)}))", "U(()=>Function.prototype.apply.call(Math.max,null,{length:Q('a',1,1),0:3}))",
  "U(()=>JSON.stringify({a:Q('a',1,'x')}))", "U(()=>JSON.stringify([Q('a',1,'x')]))", "U(()=>JSON.stringify(Q('a',1,'x')))", "U(()=>JSON.stringify({toJSON:Q('a',1,'x')}))", "U(()=>JSON.stringify({a:1},null,Q('a',2,'x')))", "U(()=>JSON.stringify({a:1},null,TP('a',2)))", "U(()=>JSON.stringify({a:1},null,Object(2)))",
  "U(()=>JSON.stringify({a:1},null,Object('--')))", "U(()=>JSON.stringify({a:Object(1n)}))", "U(()=>JSON.stringify({a:Object(Symbol())}))", "U(()=>JSON.stringify({a:Object('s'),b:Object(1),c:Object(true)}))", "U(()=>JSON.stringify(Object(1),(k,v)=>v))", "U(()=>JSON.stringify({[Q('a',1,'k')]:1}))",
  "U(()=>JSON.stringify({a:1,b:2},[Q('a','a',1),Object('b'),Object(1)]))", "U(()=>JSON.stringify({a:1,b:2,1:3},['a',1,'1',Symbol()]))", "U(()=>JSON.parse(Q('s','[1]',1)))", "U(()=>JSON.parse(Q('s',{},'[2]')))", "U(()=>JSON.parse(TP('s',null)))", "U(()=>JSON.parse(TP('s',12)))",
  "U(()=>JSON.parse('[1]',Q('r',1,1)))", "U(()=>JSON.parse('1',TP('r',1)))", "U(()=>`${Symbol.prototype[Symbol.toPrimitive].call(Object(Symbol('w')),'string')}`)",
);

// ---- 17. Object(sym) e Object(bigint) em operadores.
const wrapVals = ["Object(Symbol('w'))", "Object(Symbol.iterator)", "Object(1n)", "Object(0n)", "Object(2n**64n)", "Object(Symbol.for('f'))"];
const wrapOps = ["($)+''", "($)+1", "+($)", "-($)", "~($)", "($)*1n", "($)==($)", "($)===($)", "($)<($)", "($)<=($)", "!($)", "!!($)", "typeof($)", "String($)", "Number($)", "BigInt($)", "Boolean($)", "Object($)===($)", "($)==Object($)", "JSON.stringify($)", "[$]+''", "`${$}`",
  "($).toString()", "($).valueOf()", "($).description", "($).constructor.name", "Object.keys($).length", "Reflect.ownKeys($).length", "Object.getPrototypeOf($)===Object.getPrototypeOf(Object($.valueOf()))", "($) instanceof Object", "Object.prototype.toString.call($)", "({[$]:1})[$]",
  "({[$]:1})[$.valueOf()]", "Reflect.ownKeys({[$]:1}).map(String)+''", "Object.assign({},{[$]:1})[$]", "new Set([$,$.valueOf()]).size", "new Map([[$,1]]).get($.valueOf())", "[$].includes($.valueOf())", "[$.valueOf()].includes($)", "[$].indexOf($)", "Object.is($,$.valueOf())", "Object.is($,$)", "isNaN($)", "Math.abs($)", "Symbol.keyFor($)",
  "Object.getOwnPropertyNames($).join()", "$.hasOwnProperty('description')", "'description' in $", "Symbol.prototype.valueOf.call($)===$.valueOf()", "{$.x=1;return $.x}", "{var o=$;o++;return o}", "{var o=$;o+='a';return o}", "{var o=$;return o??1}"];
for (const w of wrapVals) for (const op of wrapOps) add(op.startsWith("{") ? T(op.split("$").join(w)) : T(op.split("$").join(w)));

// ---- 18. Reflect.ownKeys / chaves de classe com símbolo, bigint como chave.
const keyVals = ["1n", "0n", "-1n", "2n**64n", "Object(1n)", "1", "-0", "1.5", "'1'", "true", "null", "undefined", "Symbol.iterator", "[1]", "[1,2]", "{}", "NaN", "Infinity", "2**32", "2**32-1", "2**53", "-1", "1e21", "1e-7", "0.000001", "'01'", "''", "' 1'", "1.0"];
for (const k of keyVals) add(T(`({[${k}]:'v'})`), T(`Reflect.ownKeys({[${k}]:'v'}).map(x=>typeof x+':'+String(x)).join()`), T(`({a:1,[${k}]:2,b:3})[${k}]`), T(`Object.keys({b:1,[${k}]:2,a:3}).join()`), T(`{var o={};o[${k}]=1;return Reflect.ownKeys(o).map(String)+''}`),
  T(`${k} in {}`), T(`[0,1,2,3][${k}]`), T(`'abc'[${k}]`), T(`{var a=[];a[${k}]=5;return a.length+','+Reflect.ownKeys(a).map(String)}`), T(`{var o={[${k}]:1};return JSON.stringify(Object.getOwnPropertyDescriptor(o,${k}))}`), T(`Object.hasOwn({1:1,null:2,undefined:3,true:4},${k})`),
  T(`{var t=new Uint8Array(4);t[${k}]=7;return t.join()+','+Reflect.ownKeys(t)}`), T(`{class C{static [${k}]=1};return Reflect.ownKeys(C).map(String)+''}`), T(`{var m=new Map([[${k},1]]);return m.get(${k})}`), T(`{var o={[${k}]:1};return Object.entries(o).join()}`));

// ---- Execução.
const baseSources = [];
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const file of fs.readdirSync(goldenDir)) {
  if (!file.endsWith(".tsv") || file === "bigint_symbol_bun.tsv") continue;
  for (const line of fs.readFileSync(path.join(goldenDir, file), "utf8").split("\n")) {
    if (!line) continue;
    try { baseSources.push(JSON.parse(line.split("\t")[0])); } catch (e) {}
  }
}
const baseSet = new Set(baseSources);

const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida.
  const helper = /\b(Q|TP|U)\(/.test(expr) ? LOGGER : "";
  const source = '"use strict";\n' + PRELUDE + helper + `globalThis.R = ${expr}`;
  if (baseSet.has(source)) { dup++; continue; }
  jobs.push({ expr, source });
}

const PRELOAD = writeResultPreload();
const run = job =>
  new Promise(resolve => {
    // Processo fresco por programa: o JSC reifica a tabela estática de Symbol, JSON, Reflect etc. por ordem de acesso.
    // O resultado sai pelo preload em JSON (surrogate solitário vira \udXXX, sem perda no pipe UTF-8).
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => {
      const text = decodeResult(out);
      if (code !== 0 || text === null) return resolve({ ok: false, err });
      // Mesmo corte de gen-bigint-bun-golden.js: acima de 4000 unidades UTF-16 só o comprimento é registrado.
      resolve({ ok: true, out: text.length > 4000 ? "#len" + text.length : text });
    });
    child.stdin.end(job.source);
  });

async function main() {
  if (process.env.COUNT) { console.error(jobs.length); return; }
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(4, os.cpus().length) }, async () => {
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
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || /[\u2013\u2014]/.test(line.replace(/\\u201[34]/g, "X")) || /\\u201[34]/.test(line)) {
      dropped++;
      process.stderr.write("caminho, marca ou travessão: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(line);
  });
  process.stdout.write(emitFactoredLines("bigint_symbol", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
