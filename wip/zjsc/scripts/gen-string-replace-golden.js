// Gera tests/golden/string_replace_bun.tsv: String.prototype.replace/replaceAll com padrões especiais de substituição
// (`$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn`, `$<nome>`), função substituta (argumentos, grupos nomeados, tipos devolvidos),
// padrão string vazio, repetido e com surrogates, regex global/sticky/unicode, `Symbol.replace` customizado, TypeError de
// replaceAll com regex não global, mais `at`, `normalize` e `localeCompare` combinados com essas operações, medido no bun 1.4.2.
// Complementa string_bun.tsv, string_method_edge_bun.tsv e string_unicode_more_bun.tsv sem repetir seus casos.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-regexp-v-golden.js.
// O programa roda por `vm.runInThisContext` (ProgramExecutable do JSC puro) e `globalThis.R` é capturado.
// Uso: bun scripts/gen-string-replace-golden.js > tests/golden/string_replace_bun.tsv
const vm = require("node:vm");
const { emitRow } = require("./golden-prelude.js");

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":' +
  'Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const t = body => add(`T(()=>${body})`);

// ---- 1. Padrões de substituição com string como padrão.
const repls = [
  "$$", "$$$", "$$&", "$&", "$&$&", "[$&]", "$`", "$'", "[$`|$']", "$1", "$01", "$10", "$00", "$0", "$<a>", "$<", "$<>", "$",
  "$$1", "$ ", "$x", "$-", "$&&", "$'$`", "\\$&", "$$$&$$", "$$`", "$$'", "$1$&", "$11", "$100", "$a", "$<a", "<$>", "$$$",
  "", "x", "$&$", "$`$`", "$'$'",
];
const subjects = ["abc", "abcabc", "aaa", "", "a-b-c", "x"];
const pats = ["b", "a", "", "abc", "z", "aa", "c"];
for (const r of repls) {
  for (const s of ["abc", "abcabc"]) {
    for (const p of ["b", "", "abc", "z"]) {
      t(`${q(s)}.replace(${q(p)},${q(r)})`);
      t(`${q(s)}.replaceAll(${q(p)},${q(r)})`);
    }
  }
}
for (const s of subjects) {
  for (const p of pats) {
    t(`${q(s)}.replace(${q(p)},"[$&]")`);
    t(`${q(s)}.replaceAll(${q(p)},"[$&]")`);
    t(`${q(s)}.replaceAll(${q(p)},"<$\`|$'>")`);
    t(`${q(s)}.replace(${q(p)},"<$\`|$'>")`);
    t(`${q(s)}.replaceAll(${q(p)},"")`);
  }
}
// replaceAll com padrão vazio: uma inserção por posição, inclusive no fim e entre surrogates.
for (const s of ["", "a", "ab", "abc", "\u{1F600}", "a\u{1F600}b", "\u00e9", "e\u0301"]) {
  t(`${q(s)}.replaceAll("","-")`);
  t(`${q(s)}.replaceAll("","$&")`);
  t(`${q(s)}.replaceAll("",(m,i,x)=>"<"+i+">")`);
  t(`${q(s)}.replace("","-")`);
  t(`${q(s)}.replace(/(?:)/g,"-")`);
  t(`${q(s)}.replace(/(?:)/gu,"-")`);
  t(`${q(s)}.replace(/(?:)/gv,"-")`);
  t(`${q(s)}.replaceAll(/(?:)/g,(m,i)=>i)`);
  t(`${q(s)}.replaceAll(/(?:)/gu,(m,i)=>i)`);
  t(`${q(s)}.replace(/(?:)/y,"-")`);
  t(`${q(s)}.replace(/(?:)/gy,"-")`);
}
// Sobreposição: ocorrências não se sobrepõem.
for (const [s, p] of [["aaaa", "aa"], ["aaa", "aa"], ["ababab", "aba"], ["abcabc", "bca"], ["aaaa", "aaa"], ["xxxx", "xx"]]) {
  t(`${q(s)}.replaceAll(${q(p)},"[$&]")`);
  t(`${q(s)}.replaceAll(${q(p)},(m,i)=>"<"+i+">")`);
  t(`${q(s)}.replace(new RegExp(${q(p)},"g"),"[$&]")`);
}

// ---- 2. Padrões com grupos de captura e regex.
const groupCases = [
  ["(a)(b)(c)", "abc"], ["(a)(b)?", "a"], ["(a)|(b)", "b"], ["(a)|(b)", "a"], ["(?<x>a)(?<y>b)", "ab"], ["(?<x>a)|(?<y>b)", "b"],
  ["(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)", "abcdefghij"], ["(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)", "abcdefghijk"], ["(a)", "a"], ["a", "a"],
  ["(?<a>.)(?<b>.)", "xy"], ["(?<a>x)\\k<a>", "xx"], ["(x)?(y)?(z)?", "y"], ["(?:(a)|b)+", "ab"], ["(a*)", "aaa"], ["(a*)*", "aaa"],
];
const groupRepls = ["$1", "$2", "$3", "$01", "$02", "$10", "$11", "$<x>", "$<y>", "$<a>", "$<b>", "$<z>", "$<x", "$&", "$`$'", "$1$2$3", "[$1|$2]", "$0", "$00", "$99", "$010", "$$1", "$1$", "$<x>$<y>"];
for (const [p, s] of groupCases) {
  for (const r of groupRepls) {
    t(`${q(s + "!")}.replace(new RegExp(${q(p)}),${q(r)})`);
    t(`${q("^" + s + "$")}.replace(new RegExp(${q(p)},"g"),${q(r)})`);
  }
  t(`${q(s + "-" + s)}.replaceAll(new RegExp(${q(p)},"g"),(...a)=>JSON.stringify(a))`);
  t(`${q(s + "-" + s)}.replace(new RegExp(${q(p)}),(...a)=>JSON.stringify(a))`);
  t(`${q(s)}.replace(new RegExp(${q(p)},"d"),(...a)=>a.length+":"+typeof a[a.length-1])`);
}
// Grupos nomeados: `$<name>` sem grupos nomeados é literal, com é substituição (indefinido vira vazio).
for (const r of ["$<a>", "$<b>", "$<>", "$<a", "$<a>$<a>", "<$<a>>", "$<a b>", "$<A>"]) {
  t(`"xy".replace(/(?<a>x)/,${q(r)})`);
  t(`"xy".replace(/(x)/,${q(r)})`);
  t(`"xy".replace("x",${q(r)})`);
  t(`"xy".replace(/(?<a>z)?x/,${q(r)})`);
  t(`"xy".replace(/(?<a>x)|(?<b>y)/g,${q(r)})`);
}

// ---- 3. Função substituta: argumentos, tipos devolvidos, this e ordem.
const fnBodies = [
  "(m)=>m.toUpperCase()", "(m,i)=>i", "(m,i,s)=>s", "(m,i,s)=>s.length", "(...a)=>a.length", "(...a)=>JSON.stringify(a)",
  "()=>undefined", "()=>null", "()=>1", "()=>-0", "()=>1n", "()=>true", "()=>({})", "()=>[1,2]", "()=>({toString(){return 'ts'}})",
  "()=>({valueOf(){return 'vo'},toString(){return 'ts'}})", "()=>Symbol('s')", "()=>''", "()=>'$&'", "()=>'$$'", "()=>'$1'",
  "function(){return typeof this}", "function(){'use strict';return typeof this}", "function(){return this===globalThis}",
  "()=>{throw new Error('boom')}", "(m,i)=>'['+i+']'", "()=>NaN", "()=>1e21", "()=>0.1",
];
for (const f of fnBodies) {
  t(`"abcabc".replace("b",${f})`);
  t(`"abcabc".replaceAll("b",${f})`);
  t(`"abcabc".replace(/b/,${f})`);
  t(`"abcabc".replace(/(b)(c)?/g,${f})`);
  t(`"abc".replaceAll("",${f})`);
  t(`"abc".replace(/(?<n>b)/,${f})`);
}
t(`(()=>{var o=[];"abcabc".replaceAll("b",(m,i)=>{o.push(i);return "x"});return o})()`);
t(`(()=>{var o=[];"abcabc".replace(/b/g,(m,i)=>{o.push(i);return "x"});return o})()`);
t(`(()=>{var o=[];"ab".replace(/(a)(b)/,(...a)=>{o.push(a.length);return ""});return o})()`);
t(`(()=>{var o=[];"ab".replace(/(?<x>a)(?<y>b)/,(...a)=>{o.push(Object.keys(a[a.length-1]).join());return ""});return o})()`);
t(`(()=>{var o=[];"ab".replace(/(?<x>a)(?<y>z)?/,(...a)=>{o.push(Object.entries(a[a.length-1]));return ""});return o})()`);
t(`(()=>{var g;"ab".replace(/(?<x>a)/,(...a)=>{g=a[a.length-1];return ""});return [Object.getPrototypeOf(g),Object.isExtensible(g)]})()`);
t(`(()=>{var re=/a/g;"aaa".replace(re,()=>{re.lastIndex=0;return "x"});return re.lastIndex})()`);
t(`(()=>{var re=/a/g;re.lastIndex=2;var r="aaa".replace(re,"x");return [r,re.lastIndex]})()`);
t(`(()=>{var re=/a/y;re.lastIndex=1;var r="aaa".replace(re,"x");return [r,re.lastIndex]})()`);
t(`(()=>{var re=/a/gy;re.lastIndex=1;var r="aaa".replace(re,"x");return [r,re.lastIndex]})()`);
t(`(()=>{var re=/a/y;var r="baa".replace(re,"x");return [r,re.lastIndex]})()`);
t(`(()=>{var re=/a/;re.lastIndex=5;var r="aaa".replace(re,"x");return [r,re.lastIndex]})()`);
t(`(()=>{var re=/b/g;var r="abab".replace(re,"x");return [r,re.lastIndex]})()`);

// ---- 4. replaceAll com regex: global obrigatório, flags e Symbol.replace.
for (const flags of ["", "g", "i", "gi", "y", "gy", "u", "gu", "v", "gv", "d", "gd", "s", "gs", "m", "gm"]) {
  t(`"aAa".replaceAll(new RegExp("a",${q(flags)}),"x")`);
  t(`"aAa".replace(new RegExp("a",${q(flags)}),"x")`);
}
t(`"a".replaceAll({[Symbol.match]:()=>true,flags:"g",[Symbol.replace]:(s,r)=>"custom:"+s+r},"y")`);
t(`"a".replaceAll({[Symbol.match]:()=>true,flags:"",[Symbol.replace]:(s,r)=>"custom"},"y")`);
t(`"a".replaceAll({[Symbol.match]:()=>true,flags:undefined,[Symbol.replace]:(s,r)=>"custom"},"y")`);
t(`"a".replaceAll({[Symbol.match]:()=>true,flags:null,[Symbol.replace]:(s,r)=>"custom"},"y")`);
t(`"a".replaceAll({[Symbol.match]:()=>true,flags:"x",[Symbol.replace]:(s,r)=>"custom"},"y")`);
t(`"a".replaceAll({[Symbol.match]:()=>false,[Symbol.replace]:(s,r)=>"custom"},"y")`);
t(`"a".replaceAll({[Symbol.replace]:(s,r)=>"custom:"+s+r},"y")`);
t(`"a".replace({[Symbol.replace]:(s,r)=>"custom:"+s+r},"y")`);
t(`"a".replace({[Symbol.replace]:null,toString(){return "a"}},"y")`);
t(`"a".replace({[Symbol.replace]:undefined,toString(){return "a"}},"y")`);
t(`"a".replace({[Symbol.replace]:1},"y")`);
t(`"a".replace({[Symbol.replace]:{}},"y")`);
t(`"a".replaceAll({[Symbol.replace]:1,flags:"g"},"y")`);
t(`"a".replace(null,"y")`);
t(`"null".replace(null,"y")`);
t(`"undefined".replace(undefined,"y")`);
t(`"undefined".replace(void 0)`);
t(`"abc".replace("b")`);
t(`"abc".replace()`);
t(`"abc".replaceAll("b")`);
t(`"abc".replaceAll()`);
t(`"1".replace(1,2)`);
t(`"1".replaceAll(1,2)`);
t(`"a1b".replace(/\\d/,5)`);
t(`"a1b".replace(/\\d/,{toString(){return "T"}})`);
t(`"a1b".replace(/\\d/,null)`);
t(`"a1b".replace(/\\d/,undefined)`);
t(`"a1b".replace(/\\d/,Symbol())`);
t(`"a1b".replace(Symbol(),"x")`);
t(`"a1b".replace("1",1n)`);
t(`String.prototype.replace.call(null,"a","b")`);
t(`String.prototype.replace.call(undefined,"a","b")`);
t(`String.prototype.replaceAll.call(null,"a","b")`);
t(`String.prototype.replaceAll.call(12321,2,"x")`);
t(`String.prototype.replaceAll.call({toString(){return "aXa"}},"a","b")`);
t(`String.prototype.replace.call(true,"r","R")`);
t(`String.prototype.replace.length+","+String.prototype.replaceAll.length+","+String.prototype.replaceAll.name`);
t(`(()=>{var log=[];var p={toString(){log.push("p");return "a"},[Symbol.replace]:undefined};var r={toString(){log.push("r");return "x"}};var s={toString(){log.push("s");return "aa"}};var out=String.prototype.replaceAll.call(s,p,r);return [out,log]})()`);
t(`(()=>{var log=[];var p={get [Symbol.replace](){log.push("get");return undefined},toString(){log.push("p");return "a"}};var out="aa".replace(p,{toString(){log.push("r");return "x"}});return [out,log]})()`);
t(`(()=>{var log=[];var p={get [Symbol.match](){log.push("match");return true},get flags(){log.push("flags");return "g"},get [Symbol.replace](){log.push("replace");return ()=>"ok"}};var out="aa".replaceAll(p,"x");return [out,log]})()`);
t(`(()=>{var log=[];var p="a";var r=(m)=>{log.push(m);return "z"};var out="aaa".replaceAll(p,r);return [out,log]})()`);

// ---- 5. Surrogates, Unicode e padrões em contexto.
for (const [s, p] of [["\u{1F600}\u{1F601}", "\u{1F600}"], ["a\u{1F600}b", "\u{1F600}"], ["\u{1F600}", "\ud83d"], ["\u{1F600}", "\ude00"], ["\u{1F600}x", "x"]]) {
  if (/[\ud800-\udfff]/.test(p) && !/\p{Script=Common}|\p{Extended_Pictographic}/u.test(p) && p.length === 1) continue;
  t(`${q(s)}.replace(${q(p)},"[$&]")`);
  t(`${q(s)}.replaceAll(${q(p)},"[$&]")`);
  t(`${q(s)}.replace(${q(p)},"[$\`|$']")`);
  t(`${q(s)}.replace(new RegExp(${q(p)},"u"),"[$&]")`);
  t(`${q(s)}.replace(new RegExp(${q(p)},"g"),(m,i)=>"<"+i+">")`);
}
t(`"\\u{1F600}".replace(/./g,"x")`);
t(`"\\u{1F600}".replace(/./gu,"x")`);
t(`"\\u{1F600}".replace(/./gs,"x").length`);
t(`"\\u{1F600}".replace(/\\ud83d/g,"x").length`);
t(`"\\u{1F600}".replace(/\\ud83d/gu,"x").length`);
t(`"\\u{1F600}".replace("\\ud83d","x").length`);
t(`"a\\u{1F600}b".replace(/(?:)/gu,(m,i)=>i)`);
t(`"a\\u{1F600}b".replace(/(?:)/g,(m,i)=>i)`);
t(`"\\u00e9\\u00c9".replaceAll("\\u00e9","x")`);
t(`"e\\u0301".replaceAll("e","x")`);
t(`"e\\u0301".replace("\\u00e9","x")`);
t(`"e\\u0301".normalize().replace("\\u00e9","x")`);
t(`"\\u00e9".normalize("NFD").replace("e","x").normalize("NFC")`);
t(`"\\u00e9".normalize("NFD").replaceAll(/[\\u0300-\\u036f]/g,"")`);
t(`"\\u00c5ngstr\\u00f6m caf\\u00e9".normalize("NFD").replace(/\\p{M}/gu,"")`);
t(`"\\u00c5ngstr\\u00f6m caf\\u00e9".normalize("NFKD").replace(/\\p{M}/gu,"").length`);
t(`"\\ufb01nal".normalize("NFKC").replace("fi","FI")`);
t(`"\\ufb01nal".replace("fi","FI")`);
t(`"\\uff21\\uff22".normalize("NFKC").replaceAll(/[A-Z]/g,c=>c.toLowerCase())`);
t(`"\\u1100\\u1161".normalize("NFC").length`);
t(`"\\uac00".normalize("NFD").replaceAll(/./g,c=>c.charCodeAt(0).toString(16)+" ")`);
t(`"ß".replace("ß","SS")`);
t(`"ß".toUpperCase().replace("SS","ss")`);
t(`"İ".toLowerCase().replace(/\\u0307/,"!")`);
t(`"ǅ".replace(/ǆ/i,"x")`);
t(`"ǅ".replace(/ǆ/iu,"x")`);
t(`"ſ".replace(/s/i,"x")`);
t(`"ſ".replace(/s/iu,"x")`);
t(`"K".replace(/k/i,"x")`);
t(`"K".replace(/k/iu,"x")`);

// ---- 6. Limites e comprimentos.
t(`"a".repeat(100).replaceAll("a","bb").length`);
t(`"a".repeat(100).replaceAll("a","").length`);
t(`"a".repeat(10).replaceAll("","xyz").length`);
t(`"a".repeat(1000).replace(/a/g,"$&$&").length`);
t(`"a".repeat(1000).replace(/a/g,"$'").length`);
t(`"abc".replace("b","$'".repeat(3))`);
t(`"abc".replace("b","$$".repeat(3))`);
t(`"x".repeat(2**14).replaceAll("x","yy").length`);
t(`"abc".replaceAll("abc","$&".repeat(2**10)).length`);

// ---- 7. at, split e join combinados com replace.
for (const i of ["0", "-1", "1", "-2", "5", "-5", "NaN", "Infinity", "-Infinity", "1.9", "-0", "undefined", "'1'", "null", "true", "({})"]) {
  for (const s of ["", "a", "abc", "a\u{1F600}b", "\u00e9", "e\u0301"]) t(`${q(s)}.at(${i})`);
  t(`"abcabc".replace("c","XY").at(${i})`);
  t(`"abcabc".replaceAll("b","").at(${i})`);
}
t(`String.prototype.at.call(123,1)`);
t(`String.prototype.at.call(null,0)`);
t(`String.prototype.at.call({toString(){return "obj"}},-1)`);
t(`String.prototype.at.length+","+String.prototype.at.name`);
t(`"a,b,c".replaceAll(",","\\n").split("\\n")`);
t(`"a b  c".split(" ").map(x=>x||"_").join("-")`);
t(`"abc".split("").reverse().join("").replace("c","C")`);
t(`"{a}{b}".replace(/\\{(\\w)\\}/g,"[$1]")`);
t(`"2020-01-02".replace(/(?<y>\\d+)-(?<m>\\d+)-(?<d>\\d+)/,"$<d>/$<m>/$<y>")`);
t(`"2020-01-02".replace(/(\\d+)-(\\d+)-(\\d+)/,"$3/$2/$1")`);
t(`"John Smith".replace(/(\\w+)\\s(\\w+)/,"$2, $1")`);
t(`"aaa".replace(/a/g,(m,i)=>i)`);
t(`"abc".replace(/(?<x>b)/,"[$<x>]")`);
t(`"abc".replace(/(?<x>b)/,"[$<y>]")`);
t(`"abc".replace(/(?<x>b)(?<y>z)?/,"[$<y>]")`);
t(`"camelCaseString".replace(/[A-Z]/g,c=>"_"+c.toLowerCase())`);
t(`"snake_case_string".replace(/_(\\w)/g,(m,c)=>c.toUpperCase())`);
t(`"  trim  me  ".replace(/^\\s+|\\s+$/g,"")`);
t(`"a.b.c".replaceAll(".","$&$&")`);
t(`"a.b.c".replace(/./g,"x")`);
t(`"a.b.c".replace(/\\./g,"x")`);
t(`"$1.00".replace("$1","$$1")`);
t(`"$1.00".replace("$1","$$$1")`);
t(`"price".replace("price","$$5")`);
t(`"a+b".replaceAll("+","$&$&")`);
t(`"x".replace("x","$'$'")`);
t(`"xyz".replace("y","$'$'")`);
t(`"xyz".replace("y","$\`$\`")`);

// ---- 8. localeCompare em ordenação de resultados de replace (locale padrão, ASCII e Latin-1 simples).
const lc = ["a", "A", "b", "B", "\u00e1", "\u00c1", "a\u0301", "z", "\u00e4", "ae", "\u00e6", "", " ", "-", "1", "10", "2", "_", "~", "\u00df", "ss", "\u00e7", "c", "d", "\u00f1", "n", "o", "\u00f6", "\u0153"];
for (const a of lc) for (const b of lc) t(`${q(a)}.localeCompare(${q(b)})`);
for (const a of ["a", "A", "\u00e1", "z"]) {
  for (const b of ["a", "A", "\u00e1", "b"]) {
    t(`${q(a)}.localeCompare(${q(b)},undefined,{sensitivity:"base"})`);
    t(`${q(a)}.localeCompare(${q(b)},undefined,{sensitivity:"accent"})`);
    t(`${q(a)}.localeCompare(${q(b)},undefined,{sensitivity:"case"})`);
    t(`${q(a)}.localeCompare(${q(b)},undefined,{sensitivity:"variant"})`);
    t(`${q(a)}.localeCompare(${q(b)},"en",{caseFirst:"upper"})`);
    t(`${q(a)}.localeCompare(${q(b)},"en",{caseFirst:"lower"})`);
  }
}
for (const [a, b] of [["2", "10"], ["a2", "a10"], ["a02", "a2"], ["1.5", "1.10"]]) {
  t(`${q(a)}.localeCompare(${q(b)})`);
  t(`${q(a)}.localeCompare(${q(b)},undefined,{numeric:true})`);
  t(`${q(a)}.localeCompare(${q(b)},"en-u-kn-true")`);
}
t(`"a".localeCompare()`);
t(`"undefined".localeCompare()`);
t(`"a".localeCompare("b","xx-invalid-locale-")`);
t(`"a".localeCompare("b",undefined,{sensitivity:"nope"})`);
t(`"a".localeCompare("b",undefined,null)`);
t(`"a".localeCompare("b",null)`);
t(`"a".localeCompare("b",[])`);
t(`"a".localeCompare("b",["en","pt"])`);
t(`"\\u00e9".localeCompare("e\\u0301")`);
t(`"\\u00e9".localeCompare("e\\u0301",undefined,{sensitivity:"variant"})`);
t(`"\\u212b".localeCompare("\\u00c5")`);
t(`"\\uff21".localeCompare("A")`);
t(`"\\ufb01".localeCompare("fi")`);
t(`["b","a","C","\\u00e1","A","c"].sort((x,y)=>x.localeCompare(y))`);
t(`["b","a","C","\\u00e1","A","c"].sort()`);
t(`["\\u00e9","e","f","\\u00e8","E"].sort((x,y)=>x.localeCompare(y))`);
t(`["z","\\u00e4","a"].sort((x,y)=>x.localeCompare(y,"sv"))`);
t(`["z","\\u00e4","a"].sort((x,y)=>x.localeCompare(y,"de"))`);
t(`String.prototype.localeCompare.call(1,2)`);
t(`String.prototype.localeCompare.call(null,2)`);
t(`String.prototype.localeCompare.length+","+String.prototype.localeCompare.name`);

// ---- 9. normalize com argumentos esquisitos, combinado.
for (const f of ["'NFC'", "'NFD'", "'NFKC'", "'NFKD'", "undefined", "'nfc'", "'NFX'", "''", "null", "1", "{toString(){return 'NFD'}}", "['NFC']", "Symbol()"]) {
  for (const s of ["'\\u00c5'", "'A\\u030a'", "'\\u212b'", "'\\ufb01'", "''", "'abc'"]) {
    t(`${s}.normalize(${f}).length`);
    t(`${s}.replace(/./gu,c=>c.codePointAt(0).toString(16)+",").length+${s}.normalize(${f}).length`);
  }
}
t(`String.prototype.normalize.call(null)`);
t(`String.prototype.normalize.length+","+String.prototype.normalize.name`);
t(`"\\u1e9b\\u0323".normalize("NFKC")+"|"+"\\u1e9b\\u0323".normalize("NFKD").length`);

// ---- Saída.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  if (/\bsetTimeout\b|\bprocess\b|\brequire\b|\bBun\b|\bconsole\b/.test(expr)) continue;
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${/^T\(/.test(expr) ? expr : `T(()=>{return ${expr}})`}`;
  let result;
  try {
    globalThis.R = undefined;
    vm.runInThisContext(source);
    result = String(globalThis.R);
  } catch (e) {
    dropped++;
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    continue;
  }
  // Surrogate solto não cabe em String do Rust: o harness não consegue carregar a fonte nem o resultado.
  if (/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(source + result) || /\\ud[89ab][0-9a-f]{2}(?!\\ud[c-f])|(?<!\\ud[89ab][0-9a-f]{2})\\ud[c-f][0-9a-f]{2}/i.test(source + result)) {
    dropped++;
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
