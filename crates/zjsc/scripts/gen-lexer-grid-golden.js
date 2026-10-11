// Gera tests/golden/lexer_grid_bun.tsv (formato fatorado, prelúdio em lexer_grid.preludes.json): o léxico dos literais em
// grade, medido no bun 1.4.2. Cada caso é um trecho de código avaliado por `(0,eval)` ou `new Function`, com o
// SyntaxError capturado e o texto exato da mensagem no resultado.
// Grade: escapes de string (\x, \u, \u{}, octais legados, \8 \9, continuação de linha, LS/PS, terminadores crus) em
// string com aspas simples e duplas e em template (cozido e cru), sloppy x strict; literais numéricos (separadores,
// 0b/0o/0x, octal legado, BigInt, ponto, expoente, membro de literal); identificadores com escapes unicode e palavras
// reservadas escapadas; espaços Unicode, BOM e terminadores de linha em cada posição; comentários HTML-like; hashbang.
// Cada caso roda num bun filho próprio (no máximo 8 em paralelo, 5 s de limite); programa igual a um já presente em
// tests/golden/*.tsv é descartado.
// Uso: bun scripts/gen-lexer-grid-golden.js
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, GOLDEN_DIR, writeResultPreload, decodeResult } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return "\\""+Array.from({length:v.length},(_,i)=>{var c=v.charCodeAt(i);return c>31&&c<127&&c!==34&&c!==92?v[i]:"\\\\u"+(c+65536).toString(16).slice(1)}).join("")+"\\"";' +
  'if(t==="bigint")return String(v)+"n";if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const STRICT = '"use strict";';
const LF = "\n", CR = "\r", LS = "\u2028", PS = "\u2029";

// Texto ASCII puro: tudo fora de 32..126 vira \uXXXX, para o programa não carregar LS/PS/BOM crus.
function esc(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + (c.charCodeAt(0) + 65536).toString(16).slice(1));
}

const exprs = []; // textos de expressão do programa (lado direito de globalThis.R =)
const evalCase = (code) => exprs.push(`T(()=>(0,eval)(${esc(code)}))`);
const bothModes = (code) => { evalCase(code); evalCase(STRICT + code); };
const funcCase = (body) => exprs.push(`T(()=>new Function(${esc(body)})())`);

// 1. Escapes de string e template.
const escapes = [];
for (const h of ["41", "4", "4g", "g1", "", "A", "ZZ", "0", "ff", "FF", "7f", "80", "00", "4 ", "+1", "-1", "1_"]) escapes.push("\\x" + h);
escapes.push("\\x");
for (const h of ["0041", "004", "00g1", "{41}", "{0041}", "{}", "{110000}", "{10FFFF}", "{1F600}", "{ 41}", "{41 }", "{41", "{000000041}",
  "ud83d\\ude00", "d83d", "{d83d}", "{D800}\\u{DC00}", "{-1}", "{1_0}", "{+1}", "{0x1}", "", "{g}", "{41}}", "{FFFFFFFFF}", "{0}", "{00000000000000000000}",
  "dc00", "DBFF\\uDFFF", "{1F600", "u", "00", "000", "0x41", "{4_1}", "{_41}", "{41_}", "{41,}", "{41.}"]) escapes.push("\\u" + h);
for (const o of ["0", "00", "01", "07", "08", "09", "1", "12", "123", "377", "400", "47", "8", "9", "0a", "1a", "18", "08x", "000", "0000", "2", "3", "4", "5",
  "6", "7", "78", "80", "90", "81", "99", "010", "0377", "0400", "00a", "7a", "37", "38", "477", "777", "0_"]) escapes.push("\\" + o);
for (const c of ["b", "f", "n", "r", "t", "v", "'", '"', "\\", "a", "c", "d", "e", "$", "z", "A", "/", "-", " ", "{", "}", "%", "k", "N", "p", "s", "w", "x", "u"]) escapes.push("\\" + c);
for (const t of [LF, CR, CR + LF, LS, PS, "\u0085", "\u000b", "\f", "\u00a0", "\ufeff", "\t"]) escapes.push("\\" + t);
for (const t of [LF, CR, CR + LF, LS, PS, "\u0085", "\u000b", "\f", "\t", "\ufeff", "\u00a0", "\u1680", "\u2000", "\u200b", "\u0001", "\u007f", "\u0080", "\ud7ff", "\ue000", "\uffff"]) escapes.push(t);
escapes.push("\\\\x41", "\\\\", "\\", "\\0\\0", "\\1\\0", "\\0" + LS, "\\" + LS + "\\" + PS, "\\0\\x41\\u0041\\u{41}", "\\8\\9", "\\08\\09", "\\00\\08");
for (const a of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]) for (const b of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]) escapes.push("\\" + a + b);
for (const a of [0, 1, 2, 3, 4, 5, 6, 7]) for (const b of [0, 1, 7, 8]) for (const c of [0, 3, 7, 8, 9]) escapes.push("\\" + a + b + c);
for (const a of ["\\0", "\\1", "\\8", "\\x4", "\\u{41}", "\\u0041", "\\n", "\\9"]) for (const b of ["\\0", "\\1", "\\8", "\\x4", "\\u{41}", "\\u0041", "\\n", "\\9"]) escapes.push(a + b);
const uniqueEscapes = [...new Set(escapes)];
for (const e of uniqueEscapes) {
  for (const q of ['"', "'"]) bothModes(`${q}a${e}b${q}`);
  if (!e.includes("`") && !e.includes("${")) {
    bothModes("`a" + e + "b`");
    bothModes("(s=>S([s[0],s.raw[0]]))`a" + e + "b`");
  }
}
for (const q of ['"', "'"]) {
  for (const e of ["\\01", "\\8", "\\9", "\\0", "\\00", "\\1", "\\x41", "\\08"]) {
    evalCase(`${q}${e}${q};${q}use strict${q};1`);
    evalCase(`function f(){${q}${e}${q};${q}use strict${q};return 1}`);
    evalCase(`function f(){${q}use strict${q};${q}${e}${q};return 1}`);
    evalCase(`function f(){${q}use strict${q};return ${q}${e}${q}}`);
    evalCase(`(function(){${q}${e}${q};${q}use strict${q}})`);
    evalCase(`(()=>{${q}${e}${q};${q}use strict${q}})`);
    evalCase(`function f(a=1){${q}${e}${q};${q}use strict${q}}`);
    evalCase(`({m(){${q}${e}${q};${q}use strict${q}}})`);
    evalCase(`class C{m(){${q}${e}${q}}}`);
    evalCase(`class C extends (${q}${e}${q}){}`);
    evalCase(`(function(){${q}use strict${q}; return eval(${esc(`${q}${e}${q}`)})})()`);
    evalCase(`(function(){return eval(${esc(`${q}use strict${q};${q}${e}${q}`)})})()`);
    evalCase(`switch(1){case ${q}${e}${q}:}`);
    evalCase(`({${q}${e}${q}:1})`);
    evalCase(`({${q}${e}${q}(){}})`);
    evalCase(`class C{${q}${e}${q}(){}}`);
    evalCase(`class C{static ${q}${e}${q}=1}`);
    evalCase(`var o={get ${q}${e}${q}(){return 1}};1`);
    evalCase(`import(${q}${e}${q})`);
    evalCase(`/a/${q}${e}${q}`);
  }
}

// 2. Literais numéricos.
const numbers = [
  "1_000", "1__0", "1_", "_1", "0_1", "0x_1", "0x1_f", "0b1_0", "0o7_7", "1_0.5_0", "1._5", "1_.5", "1e1_0", "1e_1", "1e+1_0", "1e1_", ".5e-3", "5.e2",
  "0.0_1", "0b2", "0b", "0x", "0o8", "0o", "0B11", "0O17", "0XfF", "017", "018", "019", "08", "09", "08.5", "07.5", "0_8", "00", "0_0", "010_0", "1n", "0n",
  "0x1fn", "0b101n", "0o7n", "1_0n", "1__0n", "1.5n", "1e3n", "01n", "08n", ".5n", "1n_", "0xn", "123456789012345678901234567890n", "0b_1", "0o_1",
  "1..toString()", "1.0.toString()", "1.toString()", "1 .toString()", "1.5.toString()", "0x10.toString()", "0b1.toString()", "1e3.toString()", "5..toFixed(2)",
  "1_0..toString()", "3in[]", "3in{}", "3 in{}", "0x1g", "1a", "1_a", "0b12", "0o18", "1e", "1e+", "1e-", "1E5", ".e1", "0.e1", "0e1", "00.5", "09.5", "0.0.0", "1.2.3",
  "0x1.5", "1e1.5", ".5.5", "1e400", "-1e400", "9007199254740993", "0xFFFFFFFFFFFFFFFFF", "0b" + "1".repeat(60), "0.1+0.2", "5e-324", "2e-324", "0.000001",
  "0.0000001", "1e21", "123456789012345680000", "0x7fffffffffffffff", "0.", ".0", "0.0", "1.", "-0", "-0.0", "0e0", "0E0", "0x0", "0X0", "0B0", "0O0", "00n", "0_0n",
  "1_000_000", "1_000.000_1", "0xdead_beef", "0xDEAD_BEEFn", "0b1111_0000", "0o1_7", "1e0_1", "1e-0_1", "012", "0128", "0127", "0o12", "012n", "1n+1", "1n+1n",
  "2n**64n", "0x1n", "0b1n", "1_1n", "10n", "9n", ".1", ".1_1", "._1", "1.e", "1.e1", "1.E1", "1.e+1", "1.e-1", "0.5e+1", "0.5E-1", "1_0e1_0", "0x1_2_3", "0x1__2", "0b1__0",
  "0o1__0", "1e1__0", "1__0.5", "1.5__0", "08.", "08e1", "08_1", "07e1", "0.1_", "0._1", "0_.1", "0e_1", "0x_", "0x1_", "0b1_", "0o1_", "0e", "0x1e", "0x1e+1", "0b1e1",
  "1\u00a0", "1\u2028+1", "1/**/+1", "1//x\n", "1;2", "(1)", "[1,2]", "+1", "-1", "!1", "~1", "++1", "1++", "1--", "1=1", "1+=1", "-017", "+017", "-0b1", "-0x1", "-1n",
  "-0n", "+1n", "~1n", "!0n", "0n?1:2", "typeof 1n", "1n==1", "1n===1", "1n<2", "1n<<2n", "BigInt(1)+1n", "Math.max(0b11,0o7,0xf)", "0.1*3", "1/3", "-1/3", "2**53+2", "0xe+1",
  "0xe-1", "1e1-1", "1e+1+1", "5%2", "07+1", "08+1", "09.1+1", "0b11.5", "0o7.5",
];
const sepBases = ["12345", "0x1f2e", "0b1010", "0o1234", "12.345", "1.5e10", "12n", "0x1fn", "1e+10", "0.0012"];
for (const base of sepBases) {
  for (let i = 0; i <= base.length; i++) numbers.push(base.slice(0, i) + "_" + base.slice(i));
  for (let i = 1; i < base.length; i++) numbers.push(base.slice(0, i) + "__" + base.slice(i));
}
const uniqueNumbers = [...new Set(numbers)];
for (const lit of uniqueNumbers) {
  bothModes(lit);
  bothModes(`[${lit}]`);
  bothModes(`typeof (${lit})`);
  funcCase(`return ${lit}`);
}
for (const lit of ["017", "08", "09.5", "0_1", "00", "019", "0777", "1_0", "10n", "01n", "0x1f"]) {
  bothModes(`var x=${lit};x`);
  bothModes(`(function(){return ${lit}})()`);
  bothModes(`({[${lit}]:1})`);
  bothModes(`({${lit}:1})`);
  bothModes(`class C{${lit}(){}}`);
  bothModes(`switch(${lit}){case ${lit}:1}`);
  bothModes(`x=>${lit}`);
  bothModes(`eval(${esc(lit)})`);
  bothModes(`parseInt(${esc(lit)})+Number(${esc(lit)})`);
  bothModes(`"use strict";${lit}`);
  bothModes(`function f(){"use strict";return ${lit}}`);
  bothModes(`function f(){return ${lit};"use strict"}`);
}

// 3. Identificadores com escapes e palavras reservadas escapadas.
bothModes("var \\u0061b=1;ab");
bothModes("var a\\u0062=2;ab");
bothModes("var \\u{61}=3;a");
bothModes("var \\u{0061}=3;a");
bothModes("var \\u00e9=4;\u00e9");
bothModes("var \u00e9=4;\\u00e9");
bothModes("var \\u{1d7d8}=1");
bothModes("var a\\u{1d7d8}=1;a\u{1d7d8}");
bothModes("var \\u0030=1");
bothModes("var a\\u0030=1;a0");
bothModes("var \\u2118=1;\u2118");
bothModes("var \\u200c=1");
bothModes("var a\\u200c=1;1");
bothModes("var a\\u200d=1;1");
bothModes("var a\u200c=1;1");
bothModes("var a\u200d=1;1");
bothModes("var a\u200b=1;1");
bothModes("var \u200c=1");
bothModes("var \\u0024=1;$");
bothModes("var \\u005f=1;_");
bothModes("var \\u{24}=1;$");
bothModes("var \\u0020a=1");
bothModes("var a\\u0020=1");
bothModes("var \\u=1");
bothModes("var \\u{=1");
bothModes("var \\u{}=1");
bothModes("var \\u{110000}=1");
bothModes("var \\ud83d\\ude00=1");
bothModes("var \\u{d83d}=1");
bothModes("var \\x61=1");
bothModes("var a\\=1");
bothModes("var \\=1");
bothModes("var \\u0061\\u0062=1;ab");
bothModes("var a=1;\\u0061");
bothModes("var a=1;\\u{61}+\\u0061");
bothModes("var a=1;typeof \\u0061");
bothModes("var a=1;a\\u");
bothModes("var o={\\u0061:1};o.a");
bothModes("var o={a:1};o.\\u0061");
bothModes("var o={a:1};o.a\\u0062");
bothModes("var o={a:1};o?.\\u0061");
bothModes("var o={\\u0061};1");
bothModes("var a=1;({\\u0061})");
bothModes("class A{\\u0061(){return 1}};new A().a()");
bothModes("class \\u0061{};1");
bothModes("class C{#\\u0061=1;g(){return this.#a}};new C().g()");
bothModes("class C{#a=1;g(){return this.#\\u0061}};new C().g()");
bothModes("class C{# a=1}");
bothModes("#!");
bothModes("label\\u0031:1");
bothModes("\\u0061: 1");
bothModes("\\u0061\\u0062\\u0063: 1");
bothModes("a:{break \\u0061}");
bothModes("a:{break a\\u0062}");
bothModes("a:for(;;){continue \\u0061}");
bothModes("\\u0061sync function f(){}");
bothModes("async function f(){}");
bothModes("var f=\\u0061sync()=>1");
bothModes("var f=\\u0061sync x=>1");
bothModes("var f=async \\u0078=>1;f");
bothModes("var \\u0061sync=1;\\u0061sync");
bothModes("var f=async function(){await 1}");
bothModes("var f=async function(){\\u0061wait 1}");
bothModes("var f=async function(){var \\u0061wait}");
bothModes("var f=async function(){aw\\u0061it 1}");
bothModes("var \\u0061wait=1;\\u0061wait");
bothModes("function*g(){yi\\u0065ld 1}");
bothModes("function*g(){var yi\\u0065ld}");
bothModes("function*g(){\\u0079ield 1}");
bothModes("function g(){var yi\\u0065ld=1;return yield}");
bothModes("var yi\\u0065ld=1;yield");
bothModes("var l\\u0065t=1;let");
bothModes("l\\u0065t x=1");
bothModes("l\\u0065t\nx=1");
bothModes("let l\\u0065t=1");
bothModes("var st\\u0061tic=1;static");
bothModes("class C{st\\u0061tic m(){}}");
bothModes("class C{static st\\u0061tic(){}}");
bothModes("class C{\\u0073tatic m(){}}");
bothModes("class C{static async *m(){}}");
bothModes("class C{static \\u0061sync m(){}}");
bothModes("class C{static as\\u0079nc m(){}}");
bothModes("class C{get\\u0020m(){}}");
bothModes("class C{g\\u0065t m(){}}");
bothModes("class C{s\\u0065t m(v){}}");
bothModes("({g\\u0065t m(){return 1}})");
bothModes("({s\\u0065t m(v){}})");
bothModes("({\\u0061sync m(){}})");
bothModes("({as\\u0079nc *m(){}})");
bothModes("({\\u0067et:1})");
bothModes("for(var x \\u006ff [1]);");
bothModes("for(var x o\\u0066 [1]);");
bothModes("for(var x \\u0069n {});");
bothModes("for(let \\u0078 of []);");
bothModes("for(\\u0076ar x of []);");
bothModes("for(var x of\\u0020[]);");
bothModes("var x=1;x \\u0069nstanceof Object");
bothModes("x \\u0069n {}");
bothModes("typ\\u0065of 1");
bothModes("\\u0074ypeof 1");
bothModes("\\u0076oid 0");
bothModes("\\u0064elete x.y");
bothModes("n\\u0065w Object");
bothModes("function f(){n\\u0065w.target}");
bothModes("function f(){new.t\\u0061rget}");
bothModes("function f(){new.\\u0074arget}");
bothModes("function f(){return new.target}");
bothModes("function f(){\\u0072eturn 1}");
bothModes("function f(){ret\\u0075rn}");
bothModes("\\u0074his");
bothModes("th\\u0069s");
bothModes("\\u006eull");
bothModes("nul\\u006c");
bothModes("\\u0074rue");
bothModes("fals\\u0065");
bothModes("var o={\\u0074rue:1};o.true");
bothModes("var o={true:1};o.tru\\u0065");
bothModes("if(1){}\\u0065lse{}");
bothModes("if(0){}el\\u0073e{}");
bothModes("\\u0069f(1){}");
bothModes("do{}wh\\u0069le(0)");
bothModes("sw\\u0069tch(1){}");
bothModes("switch(1){c\\u0061se 1:}");
bothModes("switch(1){d\\u0065fault:}");
bothModes("try{}c\\u0061tch(e){}");
bothModes("try{}catch(e){}fin\\u0061lly{}");
bothModes("try{}f\\u0069nally{}");
bothModes("\\u0074ry{}finally{}");
bothModes("\\u0074hrow 1");
bothModes("thr\\u006fw 1");
bothModes("\\u0077ith({}){}");
bothModes("w\\u0069th({}){}");
bothModes("\\u0064ebugger");
bothModes("d\\u0065bugger");
bothModes("\\u0063onst x=1");
bothModes("c\\u006fnst x=1");
bothModes("\\u0063lass A{}");
bothModes("class A \\u0065xtends Object{}");
bothModes("class A extends Object{constructor(){\\u0073uper()}}");
bothModes("class A extends Object{constructor(){sup\\u0065r()}}");
bothModes("class A extends Object{m(){return sup\\u0065r.x}}");
bothModes("class A{constructor(){} \\u0063onstructor(){}}");
bothModes("class A{\\u0063onstructor(){}}");
bothModes("class A{'constructor'(){} constructor(){}}");
bothModes("\\u0069mport('x')");
bothModes("import.m\\u0065ta");
bothModes("\\u0065xport var x");
bothModes("function f(){\\u0063ontinue}");
bothModes("while(0){\\u0062reak}");
bothModes("function \\u0066(){};f");
bothModes("\\u0066unction f(){}");
bothModes("func\\u0074ion f(){}");
bothModes("var f=\\u0066unction(){}");
bothModes("var f=(\\u0061)=>a;f(1)");
bothModes("var f=(\\u0061,\\u0062)=>a+b;f(1,2)");
bothModes("var f=\\u0061=>a;f(1)");
bothModes("var f=a\\u0062=>ab;f(1)");
bothModes("var f=(a)\\u003d>a");
bothModes("var f=(a)=\\u003e a");
bothModes("var f=a\n=>1");
bothModes("function f(\\u0061){return a};f(1)");
bothModes("function f(a,\\u0061){return a};f(1,2)");
bothModes("(function(\\u0065val){})");
bothModes("(function(ev\\u0061l){})");
bothModes("(function(arguments){})");
bothModes("(function(\\u0061rguments){})");
bothModes("var \\u0061rguments=1");
bothModes("var ev\\u0061l=1");
bothModes("\\u0065val=1");
bothModes("ev\\u0061l++");
bothModes("[\\u0065val]=[1]");
bothModes("({\\u0065val}=1)");
bothModes("({eval:ev\\u0061l}=1)");
bothModes("function ev\\u0061l(){}");
bothModes("try{}catch(ev\\u0061l){}");
bothModes("undefin\\u0065d");
bothModes("var undefin\\u0065d=1;undefined");
bothModes("var Na\\u004e=1;NaN");
bothModes("Infinit\\u0079");
bothModes("var o={\\u005f\\u005fproto__:1,\\u005f\\u005fproto__:2}");
bothModes("var o={__proto__:1,__proto__:2}");
bothModes("var o={__proto__:1,\"__proto__\":2}");
bothModes("var o={__proto__:null};Object.getPrototypeOf(o)");
bothModes("var o={\\u005f\\u005fproto__:null};Object.getPrototypeOf(o)");
bothModes("var o={__proto__:null};Object.getPrototypeOf(o)");
const reserved = ["break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "enum", "export", "extends", "false",
  "finally", "for", "function", "if", "import", "in", "instanceof", "new", "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var",
  "void", "while", "with", "yield", "let", "static", "implements", "interface", "package", "private", "protected", "public", "await", "async", "of", "get", "set",
  "undefined", "NaN", "Infinity", "eval", "arguments", "target", "meta", "from", "as", "constructor", "prototype", "__proto__", "x"];
const hex4 = (c) => "\\u" + (c.charCodeAt(0) + 65536).toString(16).slice(1);
for (const w of reserved) {
  const first = hex4(w[0]) + w.slice(1);
  const mid = w.length > 1 ? w.slice(0, 1) + hex4(w[1]) + w.slice(2) : first;
  const braced = "\\u{" + w.charCodeAt(0).toString(16) + "}" + w.slice(1);
  const all = [...w].map(hex4).join("");
  for (const id of [first, mid, braced, all]) {
    bothModes(`var ${id}=1`);
    bothModes(`var ${id}=1;${id}`);
    bothModes(`${id}`);
    bothModes(`typeof ${id}`);
    bothModes(`({${id}:1})`);
    bothModes(`({${id}})`);
    bothModes(`({${id}(){}})`);
    bothModes(`(${id})=>1`);
    bothModes(`function ${id}(){}`);
    bothModes(`function f(${id}){}`);
    bothModes(`var o={${w}:1};o.${id}`);
    bothModes(`class ${id}{}`);
    bothModes(`${id}:1`);
    bothModes(`try{}catch(${id}){}`);
    bothModes(`let ${id}=1`);
    bothModes(`function*g(){var ${id}=1}`);
    bothModes(`async function g(){var ${id}=1}`);
    bothModes(`async function g(){${id}=1}`);
    bothModes(`${id}=1`);
    bothModes(`[${id}]=[1]`);
    bothModes(`({a:${id}}={a:1})`);
    bothModes(`for(var ${id} in {});`);
    bothModes(`for(${id} of []);`);
    bothModes(`import ${id} from 'x'`);
    bothModes(`1 ${id} 2`);
    funcCase(`var ${id}=1;return 2`);
    funcCase(`return typeof ${id}`);
  }
}

// 4. Espaços Unicode, BOM e terminadores de linha.
const spaces = ["\u0009", "\u000b", "\u000c", "\u0020", "\u00a0", "\u1680", "\u180e", "\u2000", "\u2001", "\u2002", "\u2003", "\u2004", "\u2005", "\u2006", "\u2007",
  "\u2008", "\u2009", "\u200a", "\u200b", "\u200c", "\u200d", "\u202f", "\u205f", "\u2060", "\u3000", "\ufeff", "\u0085", "\u001c", "\u001d", "\u001e", "\u001f",
  "\u0001", "\u0000", "\u2028", "\u2029", "\n", "\r", "\r\n", "\u2061", "\u3164", "\u115f", "\u1680\u180e", "\ufeff\ufeff", "\u0008", "\u007f", "\u00ad", "\u061c", "\u200e", "\u200f",
  "\u202a", "\u202e", "\u2066", "\ufff9", "\ufffe", "\uffff"];
for (const c of spaces) {
  bothModes(`${c}1`);
  bothModes(`1${c}`);
  bothModes(`1${c}+${c}2`);
  bothModes(`var${c}x${c}=${c}7${c};${c}x`);
  bothModes(`var x${c}= 3; x`);
  bothModes(`var${c}x=1`);
  bothModes(`va${c}r x=1`);
  bothModes(`var a${c}b=1`);
  bothModes(`var a=1;var b=2;a${c}+${c}+b`);
  bothModes(`typeof${c}1`);
  bothModes(`"a${c}b"`);
  bothModes(`'${c}'`);
  bothModes("`a" + c + "b`");
  bothModes(`/a${c}b/.test("a${c}b")`);
  bothModes(`/[${c}]/.test("${c}")`);
  bothModes(`/\\s/.test("${c}")`);
  bothModes(`"${c}".trim().length`);
  bothModes(`//${c}1\n2`);
  bothModes(`/*${c}*/3`);
  bothModes(`var x=1;x${c}++;x`);
  bothModes(`var x=1;x++${c}x`);
  bothModes(`var x=1,y=2;x${c}++${c}+y`);
  bothModes(`var a=1;a${c}=>1`);
  bothModes(`var f=a${c}=>1;f(2)`);
  bothModes(`var f=(a)${c}=>1;f(2)`);
  bothModes(`1${c}.${c}toString()`);
  bothModes(`1.${c}toString()`);
  bothModes(`1${c}n`);
  bothModes(`${c}0x${c}1`);
  bothModes(`x${c}=${c}>1`);
  bothModes(`a${c}?.b`);
  bothModes(`a${c}??${c}b`);
  bothModes(`var o={a${c}:1};1`);
  bothModes(`class C{a${c}b(){}}`);
  bothModes(`class C{static${c}a(){}}`);
  bothModes(`class C{get${c}a(){}}`);
  bothModes(`({get${c}a(){return 1}}).a`);
  bothModes(`({async${c}a(){}})`);
  bothModes(`async${c}function f(){}`);
  bothModes(`async function f(){await${c}1}`);
  bothModes(`for(var x of${c}[]);`);
  bothModes(`yield${c}1`);
  bothModes(`function*g(){yield${c}1}`);
  bothModes(`function*g(){yield${c}*[1]}`);
  funcCase(`return${c}1`);
  funcCase(`return${c}\n1`);
  funcCase(`var x=1;x${c}++;return x`);
  funcCase(`throw${c}1`);
  funcCase(`a:for(;;){break${c}a}return 7`);
  funcCase(`${c}return 1`);
  funcCase(`return ${c}1${c};`);
}
const terminators = [LF, CR, CR + LF, LS, PS, "\u0085", "\u000b", "\f", "\u00a0", "\ufeff"];
for (const t of terminators) {
  bothModes(`1${t}+${t}2`);
  bothModes(`var a=1;var b=2;a${t}++${t}b;a`);
  bothModes(`var i=1;i${t}++${t}i`);
  bothModes(`//c${t}1`);
  bothModes(`/*${t}*/1`);
  bothModes(`"${t}"`);
  bothModes("`" + t + "`");
  bothModes(`(s=>S([s[0],s.raw[0]]))\`a${t}b\``);
  bothModes(`/a${t}/`);
  bothModes(`/[${t}]/`);
  bothModes(`'\\${t}'`);
  bothModes(`var s='a\\${t}b';s`);
  bothModes(`var s="a\\${t}b";s.length`);
  bothModes(`'a'${t}+${t}'b'`);
  bothModes(`x${t}=>1`);
  bothModes(`(x)${t}=>1`);
  bothModes(`async${t}function f(){}`);
  bothModes(`async${t}x=>1`);
  bothModes(`async x${t}=>1`);
  bothModes(`let${t}x=1`);
  bothModes(`let${t}[x]=[1]`);
  bothModes(`var o={};o${t}.a`);
  bothModes(`var o={a:1};o${t}?.a`);
  bothModes(`var a=1;a${t}/2/1`);
  bothModes(`a${t}`);
  bothModes(`1${t}.toString()`);
  bothModes(`new${t}Object`);
  bothModes(`new${t}.target`);
  bothModes(`function f(){new.${t}target}`);
  bothModes(`import${t}.meta`);
  bothModes(`import${t}('x')`);
  bothModes(`typeof${t}1`);
  bothModes(`void${t}0`);
  bothModes(`for(var x${t}of [1]);`);
  bothModes(`for${t}(var x of [1]);`);
  bothModes(`do ;${t}while(0)1`);
  bothModes(`do ;${t}while(0)${t}1`);
  bothModes(`if(1)${t}2`);
  bothModes(`label${t}:1`);
  bothModes(`a:${t}1`);
  bothModes(`a:for(;;){break${t}a}`);
  bothModes(`a:for(;;){continue${t}a}`);
  bothModes(`function*g(){yield${t}1}g().next().value`);
  bothModes(`function*g(){yield${t}*[1]}`);
  bothModes(`async function f(){await${t}1}`);
  bothModes(`for await${t}(x of []);`);
  funcCase(`return${t}1`);
  funcCase(`throw${t}1`);
  funcCase(`var x=1;x${t}++;return x`);
  funcCase(`var x=1;x${t}--;return x`);
  funcCase(`var x=1,y=2;x${t}+${t}y;return x${t}+y`);
  funcCase(`a:for(;;){break${t}a}return 7`);
  funcCase(`a:for(;;){continue${t}a}return 7`);
  funcCase(`for(;;){break${t}}return 7`);
  funcCase(`return${t}`);
  funcCase(`return${t};`);
  funcCase(`return 1${t}return 2`);
  funcCase(`var a=1${t}var b=2${t}return a+b`);
  funcCase(`var a=1${t}(function(){return 2})${t}return a`);
  funcCase(`var a=function(){return 1}${t}(2)${t}return 3`);
  funcCase(`${t}return 1`);
  funcCase(`/*${t}*/return 1`);
  funcCase(`//${t}return 1`);
  funcCase(`return/*${t}*/1`);
  funcCase(`return/*x*/${t}/*y*/1`);
  funcCase(`var a=1${t}++${t}a;return a`);
  funcCase(`"a\\${t}b";return 1`);
}

// 5. Comentários HTML-like e hashbang.
const html = [
  "1 <!-- x\n+2", "<!-- a\n5", "--> a\n5", "x=1\n--> c\nx", "1 --> 0", "var x=3; x --> 0", "/* c */ --> y\n7", "/*\n*/ --> y\n7", "/* */ --> y\n7", "\n--> y\n7",
  " --> y\n7", "\t--> y\n7", "\u00a0--> y\n7", "\ufeff--> y\n7", "--> y", "<!--", "<!-- x", "<!--x\n1", "1<!--x", "1 <!--x\n", "1<!-2", "1 < !--x", "1 <! --x\n2",
  "var a=1,b=2;a<!--b", "var a=1,b=2;a<!--b\nb", "var a=2;a<!--a\n", "`<!--`", "'<!--'", "'-->'", "\"-->\"", "`-->`", "/<!--/.source", "/-->/.source", "//<!--\n1", "//-->\n1",
  "/*<!--*/1", "/*-->*/1", "/*\n-->*/1", "1\r--> x\n2", "1\r\n--> x\n2", "1\u2028--> x\n2", "1\u2029--> x\n2", "1\u2028--> x\u2028 2", "1\n--> x\r2", "1\n    --> x\n2",
  "1\n/**/ --> x\n2", "1\n/*\n*/ --> x\n2", "1\n/**/ /**/ --> x\n2", "1\n/*x*/--> x\n2", "1\n/*x*/ -->x\n2", "1\n-->", "1\n--> ", "1\n-->\n2", "1\n--->\n2", "1\n- -> 2",
  "1\n-- >2", "var x=1;\nx\n-->2", "var x=2;\nx\n-->1\nx", "var x=2;x\n--\n>1", "if(1)\n--> x\n3", "{\n--> x\n}1", "function f(){\n--> x\nreturn 4}f()", "function f(){--> x\nreturn 4}f()",
  "(function(){\n--> x\nreturn 5})()", "<!-- x\n<!-- y\n6", "<!-- x\n--> y\n6", "--> x\n<!-- y\n6", "--> x\n--> y\n6", "<!--x\n-->y\n6", "<!--\n-->\n6", "<!---->\n6", "<!--->\n6", "<!--/*\n6*/", "<!--/*\n*/6",
  "/*<!--\n*/6", "/*\n<!--*/6", "x=1;x<!--x\n", "x<!-- 1", "x\n<!-- 1", "if(1)<!-- x\n2", "if(1)<!-- x\n;else 3", "for(;;)<!-- x\nbreak", "1 + <!-- x\n2", "[1,<!-- x\n2]", "({a:<!-- x\n1})",
  "a=>\n--> x\n1", "1\n--> x\n", "1\n\n--> x", "1\n\u2028--> x", "'\\\n--> x'", "`\n--> x`", "`\n--> x`.length", "(s=>s.raw[0])`\n--> x`", "/*\n*/\n--> x\n8", "/*\n*/ /*\n*/ --> x\n8",
  "/*\n*/ 1 --> x\n8", "/*\n*/1\n--> x\n8", "1/*\n*/--> x\n8", "1 /*\n*/ --> x\n8", "1 /*\n*/ <!-- x\n8", "eval('--> x\\n9')", "eval('1\\n--> x\\n9')", "eval('<!-- x\\n9')", "new Function('--> x\\nreturn 9')()",
  "new Function('<!-- x\\nreturn 9')()", "new Function('1\\n--> x\\nreturn 9')()", "new Function('x', '--> x\\nreturn 9')()", "new Function('a\\n--> x', 'return 9')()", "new Function('a /* \\n */ ', 'return 9')()",
  "new Function('/*', '*/', 'return 9')()", "new Function('a //', 'return 9')()", "new Function('a //\\n', 'return 9')()", "new Function('a,b', 'c', 'return 9')()", "new Function('a=', '1', 'return 9')()",
  "new Function('a', 'a', 'return 9')()", "new Function('a', 'a', '\"use strict\"; return 9')()", "new Function('{a}', 'return 9')({})", "new Function('a,', 'return 9')()", "new Function('...a', 'return a')(1,2)",
  "new Function('a,...b', 'return b')(1,2,3)", "new Function('...a,b', 'return 9')()", "new Function('a=1', 'return a')()", "new Function('a=1', '\"use strict\"; return a')()", "new Function('\"use strict\"', 'return 9')()",
  "new Function('a)', '{', 'return 9')()", "new Function('a){return 1}//', 'return 9')()", "new Function('', '}), (function(){')()", "new Function('}), (function(){', '')()", "new Function('a){ return 8 }, function(b', 'return 9')()",
  "new Function('/*', '*/){')", "new Function('a', '*/){')", "new Function('/*', '*/){ return 1 }')()",
];
const hashbangs = [
  "#!x\n1", "#!", "#!x", " #!x\n1", "\n#!x\n1", "/*c*/#!x\n1", "\ufeff#!x\n1", "#!x\r\n1", "#!x\r1", "#!x\u20281", "#!x\u20291", "#!/usr/bin/env x\n7", "#!\n1", "#! x\n1", "#!/*\n1", "#!/*\n*/1",
  "#!x\n#!y\n1", "1\n#!x", "1;#!x", "(#!x\n1)", "#!x\n--> y\n2", "#!x\n<!-- y\n2", "#!x\n\"use strict\";011", "#!x\n\"\\01\";\"use strict\"", "#!\\u0061\n1", "#\\u0021x\n1", "# !x\n1", "#!x\u00a01\n2",
  "#!x\u0085 1", "#!\u2028", "#!\u2028 1", "#!\u2029 1", "#!" + "x".repeat(1000) + "\n1", "#a", "#", "#!a", "#!1", "#!\"", "#!`", "#!'", "#!\\", "##!", "#\n!", "!#", "#!x\n#!", "#!x\n\n#!\n3",
  "eval('#!x\\n1')", "eval('1;#!x\\n1')", "eval(' #!x')", "new Function('#!x\\nreturn 1')()", "new Function('#!x')()", "new Function('x', '#!x')()", "new Function('#!x', 'return 1')()", "(0,eval)('#!x')",
  "eval('#!x')", "(0,eval)('#!x\\n1')", "(0,eval)('\\ufeff#!x\\n1')", "class C{#!x\n}", "({#!x\n})", "var #!x", "x.#!y", "x.#a", "class C{#a;m(){return #a in this}};new C().m()", "class C{#a;m(){return #a}}",
  "class C{#a;m(){return #a in 1}};new C().m()", "class C{#a;#a}", "class C{#constructor}", "class C{#a;static m(o){return #a in o}};C.m(new C)", "class C{#\\u0061=1;m(){return #a in this}};new C().m()",
  "class C{#\\u{61}=1;m(){return this.#a}};new C().m()", "class C{#a=1;m(){return this.# a}}", "class C{#a=1;m(){return this.#\n a}}", "class C{#a=1;m(){return this?.#a}};new C().m()", "class C{# a}", "class C{#1}", "class C{#_1=1;m(){return this.#_1}};new C().m()",
  "class C{#$=1;m(){return this.#$}};new C().m()", "class C{#\u00e9=1;m(){return this.#\u00e9}};new C().m()", "class C{#\u2118=1;m(){return this.#\u2118}};new C().m()", "class C{#\u200c=1}", "class C{#a\u200c=1;m(){return this.#a\u200c}};new C().m()",
];
for (const code of [...html, ...hashbangs]) {
  bothModes(code);
  funcCase(code);
  funcCase("return (" + code.replace(/\n/g, " ") + "\n)");
}

// Descarta programas já presentes nos goldens vizinhos.
const known = new Set(knownPrograms("lexer_grid_bun.tsv", (file) => file !== "lexer_grid_bun.tsv"));
const seen = new Set();
const jobs = [];
let dup = 0;
for (const expr of exprs) {
  const source = PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source)) continue;
  seen.add(source);
  if (known.has(source)) { dup++; continue; }
  jobs.push({ source });
}

let timeouts = 0;
let dropped = 0;

function runChild(job) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 5000);
    child.stdout.on("data", (chunk) => { out += chunk; });
    child.on("close", (code) => {
      clearTimeout(timer);
      const decoded = code === 0 && !timedOut ? decodeResult(out) : null;
      resolve({ timedOut, code: code === 0 && !timedOut && decoded === null ? -1 : code, out: decoded === null ? "" : decoded });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(job.source);
  });
}

async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  async function worker() {
    while (next < jobs.length) {
      const index = next++;
      results[index] = await runChild(jobs[index]);
    }
  }
  await Promise.all(Array.from({ length: 8 }, worker));
  const rows = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.timedOut) { timeouts++; continue; }
    if (r.code !== 0 || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun|\u2014|\u2013|^undefined$/i.test(r.out)) {
      dropped++;
      continue;
    }
    rows.push({ source: jobs[i].source, result: r.out });
  }
  fs.writeFileSync(path.join(GOLDEN_DIR, "lexer_grid_bun.tsv"), emitFactored("lexer_grid", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, timeouts de 5 s ${timeouts}, repetidos dos goldens existentes ${dup}\n`);
}
main();
