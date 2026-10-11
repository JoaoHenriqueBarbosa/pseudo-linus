// Gera tests/golden/bigint_edge_bun.tsv: BigInt de borda (literais e BigInt() de strings, asIntN/asUintN, toString em
// radix 2..36 com milhares de dígitos, operadores com negativos em complemento de dois, comparação com Number e string,
// BigInt64Array/BigUint64Array/DataView, JSON, Math, mistura de tipos, toLocaleString, parseInt/Number, divisão por zero
// e mensagens de erro) medidos no bun 1.4.2.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Cada programa é `try { R = S(expressão) } catch (e) { R = e.name + ': ' + e.message }`, rodado por vm.runInThisContext.
// Caminho da máquina no resultado descarta o programa. Cada execução tem timeout.
// Uso: bun scripts/gen-bigint-edge-golden.js > tests/golden/bigint_edge_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE =
  "var S=function(v){if(typeof v==='bigint')return v+'n';if(typeof v==='string')return JSON.stringify(v);" +
  "if(Object.is(v,-0))return '-0';if(typeof v==='symbol')return v.toString();if(typeof v==='undefined')return 'undefined';" +
  "if(Array.isArray(v))return '['+v.map(S).join(',')+']';if(typeof v==='function')return 'function';" +
  "if(v!==null&&typeof v==='object'&&!(v instanceof Number))return Object.prototype.toString.call(v)+'{'+Object.keys(v).join()+'}';return String(v)};" +
  "var D=function(s){var t=0;for(var i=0;i<s.length;i++)t+=parseInt(s[i],36);return s.length+':'+t};\n";

const programs = [];
// Expressão avaliada e formatada por S.
const E = expr => programs.push(`${PRELUDE}try { R = S(${expr}) } catch (e) { R = e.name + ': ' + e.message }`);
const q = JSON.stringify;

// ---- 1. BigInt() de strings.
const strings = [
  "0", "-0", "+0", "12", "-12", "+12", "00012", "  12  ", "\n12\t", " 12 ", "﻿12", "", " ", "\n", "0x1f", "0X1F", "0o17", "0O17", "0b101", "0B101",
  "-0x1", "+0x1", "0x", "0b2", "0o8", "0xg", "1n", "1.5", "1e3", "1_000", "Infinity", "NaN", "12 3", "--1", "+-1", "- 1", "١٢", "0x" + "f".repeat(40),
  "9".repeat(60), "-" + "9".repeat(60), "1" + "0".repeat(100), "0.0", ".5", "5.", "0x0", "0b0", "0o0", "1\u0000", "a",
];
for (const s of strings) E(`BigInt(${q(s)})`);

// ---- 2. Literais no fonte (eval indireto).
for (const src of ["0n", "0x1Fn", "0X1Fn", "0o17n", "0b101n", "1_000n", "00n", "08n", "07n", "1.5n", "1e3n", ".5n", "0xn", "1nn", "-0n", "0B1n", "1_n", "1__0n", "1n_",
  "123456789012345678901234567890n", "0n === -0n", "0xFFFFFFFFFFFFFFFFFFFFn", "1n.toString()", "1n .toString(2)", "0b1_0n", "0_1n", "1e1n", "\\u{31}n", "1\\u006e", "typeof 1n"]) {
  E(`(0, eval)(${q(src)})`);
}

// ---- 3. BigInt() de valores.
for (const v of ["1", "-1", "0", "-0", "1e21", "2**53", "2**53+2", "1.5", "NaN", "Infinity", "-Infinity", "1e300", "-1e21", "0.1", "true", "false", "null", "undefined", "Symbol()", "({})", "[]",
  "[5]", "[1,2]", '"5"', "Object(5n)", "({valueOf(){return 3}})", "({valueOf(){return 3.5}})", "({toString(){return '0x10'}})", "5n", "new Number(7)", "new String('8')", "1e-7", "2**64", "-(2**63)", "Number.MAX_VALUE",
  "Number.MIN_VALUE", "Number.MAX_SAFE_INTEGER", "({[Symbol.toPrimitive](){return 9n}})", "({[Symbol.toPrimitive](){return '9'}})", "function(){}", "new Date(5)"]) {
  E(`BigInt(${v})`);
}
E("new BigInt(1)");
E("BigInt()");
E("BigInt.length+':'+BigInt.name");
E("Object.getOwnPropertyNames(BigInt.prototype).sort().join()");
E("Object.prototype.toString.call(1n)+Object.prototype.toString.call(Object(1n))");
E("BigInt.prototype[Symbol.toStringTag]");
E("BigInt.prototype.constructor===BigInt");
E("BigInt.asIntN.length+':'+BigInt.asUintN.length");
E("Object.getPrototypeOf(Object(1n))===BigInt.prototype");

// ---- 4. asIntN / asUintN.
const bitsList = ["0", "1", "8", "63", "64", "65", "2**53-1", "2**53", "-1", "1.5", "'8'", "NaN", "undefined", "Infinity", "-0", "2**53+1"];
const valList = ["0n", "1n", "-1n", "255n", "128n", "2n**63n", "-(2n**63n)-1n", "2n**64n+5n"];
for (const fn of ["asIntN", "asUintN"]) {
  for (const b of bitsList.slice(0, 8)) for (const v of valList.slice(0, 5)) {
    if (b === "2**53" || b === "2**53-1") { if (fn === "asUintN" && v.startsWith("-")) continue; }
    E(`BigInt.${fn}(${b},${v})`);
  }
  for (const b of bitsList.slice(8)) E(`BigInt.${fn}(${b},-1n)`);
  E(`BigInt.${fn}(8,255)`); E(`BigInt.${fn}(8,'255')`); E(`BigInt.${fn}(8)`); E(`BigInt.${fn}(8,Object(300n))`); E(`BigInt.${fn}(8,true)`); E(`BigInt.${fn}(2n,1n)`);
  E(`BigInt.${fn}(64,2n**200n+12345n)`); E(`BigInt.${fn}(100,-(2n**200n)-1n)`); E(`BigInt.${fn}(Symbol(),1n)`); E(`BigInt.${fn}(65,2n**64n)`);
}
E("BigInt.asIntN(2**53-1,5n)"); E("BigInt.asUintN(2**53-1,5n)"); E("BigInt.asIntN(2**53,-5n)");

// ---- 5. toString em radix 2..36 de números gigantes (comprimento:soma de dígitos).
const bigs = ["7n**2000n", "-(3n**5000n)", "2n**20000n-1n"];
for (const b of bigs) for (let r = 2; r <= 36; r++) E(`D((${b}).toString(${r}))`);
E("(7n**2000n).toString().length"); E("(10n**5000n).toString(16).length"); E("BigInt('0x'+(7n**900n).toString(16))===7n**900n");
E("BigInt('0b'+(5n**700n).toString(2))===5n**700n"); E("BigInt('0o'+(5n**700n).toString(8))===5n**700n"); E("BigInt((9n**1500n).toString())===9n**1500n");
E("(255n).toString(16)+(-255n).toString(2)+(0n).toString(36)"); E("(35n).toString(36)+(36n).toString(36)");
for (const r of ["1", "37", "0", "-1", "null", "undefined", "'16'", "2.9", "NaN", "Infinity", "Symbol()", "1n", "true", "({valueOf(){return 8}})", "'x'"]) E(`(255n).toString(${r})`);
E("BigInt.prototype.toString.call(1)"); E("BigInt.prototype.toString.call('x')"); E("BigInt.prototype.toString.call(Object(5n),2)"); E("BigInt.prototype.valueOf.call(1)");
E("BigInt.prototype.valueOf.call(Object(3n))"); E("BigInt.prototype.toString.call({})"); E("(5n).toString.call(6n,2)"); E("String(-0n)+(0n).toString()"); E("`${-5n}${5n}`");
E("(2n**64n).toString(2).length"); E("(-(2n**64n)).toString(2).length"); E("(2n**100000n).toString(16).length");

// ---- 6. Operadores.
const lhs = ["-7n", "7n", "0n", "-1n", "2n**64n+1n", "-(2n**65n)-3n"];
const rhs = ["-3n", "3n", "2n"];
for (const op of ["/", "%", "&", "|", "^"]) for (const a of lhs) for (const b of rhs) E(`(${a})${op}(${b})`);
for (const a of ["-2n", "0n", "2n", "-1n"]) for (const b of ["-1n", "0n", "1n", "3n", "64n"]) E(`(${a})**(${b})`);
for (const a of ["5n", "-5n", "-1n", "2n**70n+1n"]) for (const b of ["-3n", "0n", "3n", "64n", "-64n"]) { E(`(${a})<<(${b})`); E(`(${a})>>(${b})`); }
for (const v of ["0n", "1n", "-1n", "255n", "-256n", "2n**64n", "-(2n**64n)", "2n**100n-1n"]) { E(`~(${v})`); E(`-(${v})`); }
for (const [a, b] of [["1n", "0n"], ["0n", "0n"], ["-1n", "0n"]]) { E(`${a}/${b}`); E(`${a}%${b}`); }
E("0n**0n"); E("0n**-1n"); E("0n**1n"); E("(-1n)**(2n**70n)"); E("(-1n)**(2n**70n+1n)"); E("1n**(2n**70n)"); E("2n**-1n"); E("(-8n)/3n"); E("(-8n)%3n"); E("8n%(-3n)");
E("5n>>>1n"); E("5n>>>0n"); E("(2n**200n)>>(2n**70n)"); E("(-(2n**200n))>>(2n**70n)"); E("1n<<(2n**70n)"); E("0n<<(2n**70n)"); E("1n>>-(2n**70n)");
E("(-5n)>>1n"); E("(-5n)>>100n"); E("5n>>100n"); E("(-1n)&(2n**64n)"); E("(-(2n**64n))|1n"); E("(-(2n**64n))^(2n**64n)"); E("(-(2n**70n))&(-(2n**65n))");
E("(2n**64n-1n)&(-(2n**32n))"); E("(2n**64n-1n)^(-1n)"); E("~~(2n**80n)"); E("-(-(2n**63n))"); E("123456789012345678901234567890n*987654321098765432109876543210n");
E("123456789012345678901234567890123n/987654321n"); E("-123456789012345678901234567890123n%987654321n"); E("(10n**40n)/(10n**20n)"); E("(10n**40n+1n)%(10n**20n)");

// ---- 7. Comparação.
for (const c of ["1n==1", "1n===1", "1n=='1'", "1n==1.5", "1n<1.5", "2n>1.5", "1n<'2'", "1n<'x'", "1n>'x'", "2n**64n==2**64", "2n**64n+1n>2**64", "2n**64n+1n==2**64", "1n<NaN", "1n>NaN", "1n<Infinity",
  "-1n>-Infinity", "0n==-0", "0n==''", "0n=='  '", "1n=='0x1'", "1n=='1n'", "1n==true", "0n==false", "2n==true", "1n==Object(1n)", "Object(1n)==Object(1n)", "Object(1n)===1n", "1n==[1]", "1n==({valueOf(){return 1n}})",
  "9007199254740993n==9007199254740992", "9007199254740993n>9007199254740992", "9007199254740993n<9007199254740994", "1n<=1", "'1'<=1n", "'a'<1n", "'1'<1n", "null<1n", "undefined<1n", "undefined==0n", "null==0n",
  "0n==null", "1n!=1", "1n!==1", "-1n<0", "-1n<-0.5", "2n**1024n>Number.MAX_VALUE", "-(2n**1024n)<-Number.MAX_VALUE", "2n**1024n==Infinity", "1n<Symbol()", "1n=='1.0'", "1n==' 1 '", "'' < 1n", "'0x10'==16n", "'1e2'==100n",
  "Object.is(0n,-0n)", "[0n].includes(-0n)", "[1n].indexOf(1)", "[1n].includes(1)", "new Set([1n,1n,1,Object(1n)]).size", "new Map([[1n,'a']]).get(1n)", "new Map([[1n,'a']]).get(1)", "[3n,1n,2n,-1n,10n].sort().join()",
  "[3n,1n,2n,-1n,10n].sort((a,b)=>a<b?-1:a>b?1:0).join()", "[3n,1n,2n].sort((a,b)=>a-b).join()", "[3n,1,2n,0.5].sort((a,b)=>a<b?-1:1).join()", "Math.max(1n,2n)"]) E(c);

// ---- 8. BigInt64Array / BigUint64Array / DataView.
for (const t of ["BigInt64Array", "BigUint64Array"]) {
  for (const v of ["2n**63n", "2n**63n-1n", "2n**64n", "2n**64n+5n", "-1n", "-(2n**63n)", "-(2n**63n)-1n", "0n", "2n**200n+7n"]) E(`${t}.of(${v})[0]`);
  E(`new ${t}([1,2])`); E(`new ${t}(3).fill(5n)`); E(`new ${t}(3).fill(5)`); E(`new ${t}(2).byteLength+':'+${t}.BYTES_PER_ELEMENT`); E(`${t}.from(['1','2'])`);
  E(`${t}.from([1n,2n]).join()`); E(`new ${t}([3n,1n,2n]).sort().join()`); E(`new ${t}([3n,1n,-2n]).sort().join()`); E(`new ${t}([1n,2n]).includes(1)`); E(`new ${t}([1n,2n]).indexOf(2n)`);
  E(`new ${t}([1n]).map(x=>x+1)`); E(`new ${t}([1n]).map(x=>x+1n).join()`); E(`new ${t}(2).set([1n,2n])`); E(`new ${t}(2).set([1,2])`); E(`new ${t}([1n,2n,3n]).subarray(1).join()`);
  E(`JSON.stringify(new ${t}([1n]))`); E(`Object.keys(new ${t}([1n,2n])).join()`); E(`new ${t}(new ArrayBuffer(16)).length`); E(`new ${t}(new ArrayBuffer(12))`);
  E(`(function(){var a=new ${t}(1);a[0]='7';return a[0]})()`); E(`(function(){var a=new ${t}(1);a[0]=7;return a[0]})()`); E(`(function(){var a=new ${t}(1);a[0]=Object(9n);return a[0]})()`);
  E(`(function(){var a=new ${t}(1);a[0]=true;return a[0]})()`); E(`(function(){var a=new ${t}(1);a[0]={valueOf(){return 4n}};return a[0]})()`); E(`(function(){var a=new ${t}(1);a[5]=1;return a[5]})()`);
  E(`Atomics.add(new ${t}(1),0,5n)`); E(`Atomics.add(new ${t}(1),0,5)`); E(`(function(){var a=new ${t}(1);Atomics.store(a,0,2n**64n+3n);return Atomics.load(a,0)})()`);
  E(`new ${t}([1n,2n,3n]).reverse().join()`); E(`new ${t}([1n,2n,3n]).reduce((a,b)=>a+b)`); E(`Object.prototype.toString.call(new ${t}(0))`); E(`new ${t}([5n]).toString()`);
  E(`new ${t}([1n,2n]).slice().constructor.name`); E(`new ${t}(['1'])`); E(`new ${t}([undefined])`); E(`new ${t}([null])`); E(`new ${t}(new Int8Array(2))`); E(`new ${t}(new ${t === "BigInt64Array" ? "BigUint64Array" : "BigInt64Array"}([-1n])).join()`);
}
for (const le of ["true", "false", "undefined"]) {
  E(`(function(){var d=new DataView(new ArrayBuffer(16));d.setBigInt64(0,-2n,${le});return [d.getBigInt64(0,${le}),d.getBigUint64(0,${le}),d.getUint8(0),d.getUint8(7)].join()})()`);
  E(`(function(){var d=new DataView(new ArrayBuffer(16));d.setBigUint64(8,2n**64n-1n,${le});return [d.getBigInt64(8,${le}),d.getBigUint64(8,${le})].join()})()`);
}
E("new DataView(new ArrayBuffer(8)).setBigInt64(0,1)"); E("new DataView(new ArrayBuffer(8)).getBigInt64(1)"); E("new DataView(new ArrayBuffer(8)).setBigUint64(0,2n**64n+9n)");
E("new BigInt64Array([1n,2n]).at(-1)"); E("Array.from(new BigUint64Array([1n,2n]),x=>x*2n).join()"); E("[...new BigInt64Array([4n,5n])].join()"); E("Array.prototype.concat.call([],new BigInt64Array([1n]))");

// ---- 9. JSON.
for (const e of ["1n", "[1n]", "{a:{b:[1n]}}", "BigInt(2**53)", "0n", "-0n", "Object(1n)", "{a:1n}", "[{toJSON(){return 1n}}]"]) E(`JSON.stringify(${e})`);
E("JSON.stringify({a:1n},null,2)"); E("JSON.stringify({a:1n},['a'])"); E("JSON.stringify({a:1n},['b'])"); E("JSON.stringify({a:1n},(k,v)=>typeof v==='bigint'?String(v):v)");
E("JSON.stringify({a:1n},()=>1)"); E("JSON.stringify(1n,()=>1)"); E("JSON.stringify(Object(10n),(k,v)=>typeof v==='object'?String(v):v)");
E("(function(){BigInt.prototype.toJSON=function(){return this.toString()+'n'};try{return JSON.stringify({a:1n,b:[2n],c:Object(3n)})}finally{delete BigInt.prototype.toJSON}})()");
E("(function(){BigInt.prototype.toJSON=function(k){return typeof this+':'+k};try{return JSON.stringify({a:1n,b:[2n]})}finally{delete BigInt.prototype.toJSON}})()");
E("(function(){BigInt.prototype.toJSON=function(){return 3n};try{return JSON.stringify(1n)}finally{delete BigInt.prototype.toJSON}})()");
E("JSON.stringify(JSON.rawJSON('12345678901234567890123'))"); E("JSON.stringify({a:JSON.rawJSON(String(2n**100n))})"); E("JSON.rawJSON(1n)");
E("JSON.parse('12345678901234567890',(k,v,c)=>typeof v==='number'?BigInt(c.source):v)"); E("JSON.parse('[1,22222222222222222222]',(k,v,c)=>typeof v==='number'?BigInt(c.source):v).join()");
E("JSON.parse('1n')"); E("structuredClone(5n)"); E("structuredClone([1n,Object(2n)])");

// ---- 10. Math e funções globais.
for (const f of ["abs", "max", "min", "floor", "ceil", "round", "trunc", "sign", "sqrt", "cbrt", "pow", "hypot", "sin", "exp", "log", "fround", "clz32", "imul", "atan2"]) E(`Math.${f}(1n,2n)`);
E("(r=>typeof r+(r>=0&&r<1))(Math.random(1n,2n))"); // valor aleatório: só o tipo e o intervalo são determinísticos
E("Math.max()"); E("Math.max(1,2n)"); E("Math.abs(Object(1n))"); E("Math.max(Number(1n),Number(2n))");

// ---- 11. Mistura de tipos.
for (const m of ["1n+1", "1+1n", "1n+'1'", "'1'+1n", "1n+undefined", "1n+null", "1n+true", "1n+{}", "1n+[]", "1n+[2]", "1n+Symbol()", "1n*1.5", "1n-1", "1n/1", "1n%1", "1n**1", "1n<<1", "1n&1", "1n|1", "1n^1", "+1n", "-1n", "~1n",
  "!0n", "!!1n", "0n?1:2", "0n||5", "1n&&5", "0n??5", "typeof 1n", "typeof Object(1n)", "typeof BigInt", "typeof BigInt.prototype", "Object(1n) instanceof BigInt", "1n instanceof BigInt", "Object(1n)+1n", "Object(2n)*Object(3n)",
  "Object(1n).valueOf()===1n", "Object(1n)===Object(1n)", "({valueOf(){return 1n}})+1n", "+({valueOf(){return 1n}})", "({[Symbol.toPrimitive](h){return h==='number'?2n:'s'}})*2n", "({[Symbol.toPrimitive](h){return h}})+1n",
  "`${1n}`", "String(1n)", "1n+'a'", "'a'+1n", "[1n]+''", "[1n,2n]+''", "1n.constructor===BigInt", "(1n).__proto__===BigInt.prototype", "1n.x", "Object(1n).x=1", "Object.keys(Object(1n)).length", "Object.getOwnPropertyNames(Object(12n)).length",
  "Object(1n)+'x'", "Object(1n)<2n", "1n<Object(2n)", "isNaN(1n)", "isFinite(1n)", "Number.isFinite(1n)", "Number.isNaN(1n)", "Number.isInteger(1n)", "Number.isSafeInteger(1n)", "Number.isInteger(Number(1n))", "[1n,2n].map(x=>x*2n).join()",
  "[1n,2n].reduce((a,b)=>a+b,0n)", "[1n,2n].reduce((a,b)=>a+b,0)", "Array(3).fill(0n).join('')", "Object.is(1n,1n)", "Object.entries({a:1n})", "Object.assign({},{a:2n}).a", "0n===-0n", "1n/2n", "-1n/2n", "-7n%4n", "7n%-4n"]) E(m);
for (const body of ["let x=1n;return [x++,x]", "let x=1n;return [++x,x]", "let x=1n;return [x--,x]", "let x=1n;return [--x,x]", "let x=1n;x+='a';return x", "let x=1n;x+=1;return x", "let x=1n;x**=3n;return x", "let x=-5n;x>>=1n;return x",
  "let x=5n;x<<=2n;return x", "let x=5n;x&=3n;return x", "let x=5n;x|=3n;return x", "let x=5n;x^=3n;return x", "let x=5n;x%=3n;return x", "let x=5n;x/=2n;return x", "let x=5n;x*=2n;return x", "let x=5n;x-=7n;return x", "let x=5n;x>>>=1n;return x",
  "let x=Object(1n);x++;return typeof x+x", "let x='1';x++;return typeof x+x", "let x=1n;x=x**x;return x", "let o={a:1n};o.a++;return o.a", "let a=[1n];a[0]--;return a[0]", "let x=1n;x&&=2n;return x", "let x=0n;x||=2n;return x", "let x=0n;x??=2n;return x",
  "let n=0n;for(let i=0n;i<5n;i++)n+=i;return n", "let f=1n;for(let i=1n;i<=30n;i++)f*=i;return f", "let a=0n,b=1n;for(let i=0;i<200;i++){[a,b]=[b,a+b]}return a", "switch(1n){case 1:return 'num';case 1n:return 'big'}", "return 1n in {1:1}",
  "return 1n in [0,1]", "return ({1:'a'})[1n]", "return [5,6,7][1n]", "return 'abc'.charAt(1n)", "return 'abc'.repeat(2n)", "return [1,2,3].slice(1n)", "return new Array(2n)", "return Array(3n).length", "return 'x'.padStart(3n)", "return [..."+"'ab'].length"]) {
  E(`(function(){${body}})()`);
}

// ---- 12. toLocaleString e Intl.
for (const loc of ["en-US", "de-DE", "pt-BR", "hi-IN"]) for (const v of ["1234567890123456789n", "-1234567n", "0n", "12345678901234567890123456789012345678901234567890n"]) E(`(${v}).toLocaleString(${q(loc)})`);
E("(1234567n).toLocaleString()"); E("(1234567n).toLocaleString(undefined)"); E("(123456789n).toLocaleString('pt-BR',{style:'currency',currency:'BRL'})"); E("(123456789n).toLocaleString('en-US',{notation:'compact'})");
E("(1234567n).toLocaleString('ar-EG')"); E("(1234567n).toLocaleString('ja-JP',{useGrouping:false})"); E("(5n).toLocaleString('en-US',{minimumFractionDigits:2})"); E("(1234n).toLocaleString('de-CH')");
E("new Intl.NumberFormat('en-US').format(2n**70n)"); E("new Intl.NumberFormat('pt-BR').format(-(2n**70n))"); E("new Intl.NumberFormat('en-US').formatToParts(1234567n).map(p=>p.type+p.value).join('|')");
E("new Intl.NumberFormat('en-US',{style:'percent'}).format(5n)"); E("new Intl.NumberFormat('en-US',{maximumSignificantDigits:3}).format(123456n)"); E("new Intl.NumberFormat('en-US').format(Object(7n))");
E("BigInt.prototype.toLocaleString.call(1)"); E("new Intl.NumberFormat('en-US').formatRange(1n,5n)"); E("(123n).toLocaleString('en-US',{style:'unit',unit:'kilometer'})");

// ---- 13. parseInt, Number e conversões.
for (const c of ["parseInt(10n)", "parseInt('10n')", "parseInt(2n**70n)", "parseInt(10n,2)", "parseFloat(12n)", "parseFloat('12n')", "Number.parseInt(1n)", "Number(2n**64n)", "Number(2n**1024n)", "Number(-(2n**1024n))", "Number(2n**53n+1n)", "Number(2n**53n+3n)",
  "Number(2n**53n+2n)", "Number(-(2n**53n)-1n)", "Number(-(2n**53n)-3n)", "Number(2n**1024n-2n**971n)", "Number(2n**1024n-2n**970n)", "Number(2n**1024n-2n**970n-1n)", "Number(0n)", "Object.is(Number(-0n),0)", "Number(123456789012345678901234567890n)",
  "Number((2n**100n)+(2n**47n))", "Number((2n**100n)+(2n**47n)+1n)", "Number((2n**100n)+(2n**47n)-1n)", "Number('1n')", "Number('')", "Number(' 0x10 ')", "+'1n'", "Number(Object(5n))", "Number({valueOf(){return 5n}})", "Number(1n)===1", "Number.MAX_SAFE_INTEGER+2",
  "BigInt(Number.MAX_SAFE_INTEGER)+2n", "BigInt(2**53)+1n", "BigInt(1e21)", "BigInt(2**100)", "BigInt(-(2**100))", "BigInt(Number.MAX_VALUE)>2n**1023n", "BigInt(1e21).toString(16)", "parseInt('123456789012345678901234567890')", "Number.parseFloat('1e1000')",
  "String(2n**64n)", "(2n**64n).toString()", "Number.prototype.toString.call(1n)", "Number.prototype.valueOf.call(1n)", "(1.5).toFixed(1n)", "[1,2,3].at(1n)", "BigInt(Number('0x10'))", "BigInt(parseInt('ff',16))", "BigInt(Math.floor(5.9))", "BigInt(Math.pow(2,60))",
  "BigInt.asIntN(64,BigInt(2**63))", "Number(BigInt.asUintN(64,-1n))", "BigInt(new Date(1e12).getTime())", "BigInt(Date.UTC(2020,0,1))*1000n", "typeof BigInt(1)", "BigInt(1)===1n", "BigInt('1')===1n"]) E(c);

// ---- 14. Mensagens de erro.
for (const c of ["1n/0n", "1n%0n", "2n**-1n", "BigInt(1.5)", "BigInt('x')", "BigInt('1.5')", "BigInt(Symbol())", "BigInt(NaN)", "BigInt(Infinity)", "BigInt(undefined)", "BigInt(null)", "BigInt({})", "BigInt([1,2])", "1n+1", "+1n", "1n>>>0n", "Math.abs(1n)", "JSON.stringify(1n)",
  "Symbol()+1n", "1n+Symbol()", "new BigInt(1)", "BigInt.prototype.toString.call(1)", "(1n).toString(1)", "BigInt.asIntN(-1,1n)", "BigInt.asUintN(2**53,1n)", "BigInt.asIntN(1,1)", "1n<<1.5", "1n<<'1'", "Object(1n)+1", "BigInt('0x')", "BigInt('  ')",
  "BigInt('9'.repeat(10)+'n')", "1n.toFixed", "Atomics.add(new Int32Array(1),0,1n)", "new BigInt64Array(1).fill(1)", "[1n].sort((a,b)=>a-1)", "[2n,1n].sort((a,b)=>a-b)", "new Intl.NumberFormat().format({})", "Number.prototype.toString.call(1n)", "(1n).toLocaleString('xx-invalid-locale-')"]) E(c);

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bigint-edge-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (sem o transpilador do bun); `R` fica na global.
const source_file = path.join(dir, "case_source.js");
const file = path.join(dir, "case.js");
fs.writeFileSync(file, `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(preload, "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n");
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  if (seen.has(body)) continue;
  seen.add(body);
  const source = body.replace(/\bR = /g, "globalThis.R = ");
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(PRELUDE.length, PRELUDE.length + 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
