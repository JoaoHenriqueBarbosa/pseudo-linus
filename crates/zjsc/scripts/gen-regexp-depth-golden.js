// Gera tests/golden/regexp_depth_bun.tsv: RegExp em profundidade, medido no bun 1.4.2.
// Complementa regexp_edge, regexp_more, regexp_v e regexp_opt com matrizes sistemáticas: lookbehind da direita para a
// esquerda com capturas e backreferences, reinício de capturas em laços, laços vazios, case folding de caracteres
// especiais com i/iu/iv, coerção de lastIndex, backtracking pesado com limites pequenos, sintaxe Annex B, Symbol.match/
// replace/search/split/matchAll com subclasses e exec personalizado, descritores do protótipo, escapes de source,
// propriedades Unicode em amostras, indices (flag d) e mensagens exatas de SyntaxError.
// Cada programa roda num bun filho novo, sem APIs de host; o resultado é o valor de `globalThis.R`.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-regexp-depth-golden.js > tests/golden/regexp_depth_bun.tsv
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":Array.isArray(v)?"["+v.map(S).join(",")+"]":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'function M(m){if(m===null)return "null";var o={a:Array.from(m),i:m.index,g:m.groups===undefined?"u":Object.entries(m.groups)};' +
  'if(m.indices){o.d=Array.from(m.indices);o.dg=m.indices.groups===undefined?"u":Object.entries(m.indices.groups)}return JSON.stringify(o)}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);
const re = (p, f) => `new RegExp(${q(p)},${q(f || "")})`;
const exec = (p, f, s) => add(`T(()=>M(${re(p, f)}.exec(${q(s)})))`);

// ---- 1. Lookbehind com capturas e backreferences (avaliação da direita para a esquerda).
for (const [p, ss] of [
  ["(?<=(\\d)(\\d))x", ["12x", "1x", "x"]], ["(?<=\\1(a))b", ["aab", "ab", "b"]], ["(?<=(a)\\1)b", ["aab", "ab"]],
  ["(?<=(?<n>a)\\k<n>)b", ["aab", "ab"]], ["(?<=\\k<n>(?<n>a))b", ["aab", "ab"]], ["(?<=a(?=b))b", ["ab"]],
  ["(?<=(a+))b", ["aaab"]], ["(?<=(a+?))b", ["aaab"]], ["(?<=(a*))b", ["aaab", "b"]], ["(?<=^|,)\\w+", ["x,yy,zzz"]],
  ["(?<!a)b", ["ab", "bb", "b"]], ["(?<!(a))b", ["ab", "cb"]], ["(?<=(?<!x)a)b", ["xab", "yab", "ab"]],
  ["(?<=a|bc)d", ["bcd", "ad", "cd"]], ["(?<=(a)|(b))c", ["ac", "bc", "cc"]], ["(?<=\\b)x", ["x", "ax", " x"]],
  ["(?<=[a-c]{2})d", ["abd", "ad", "zabd"]], ["(?<=(\\w)(\\w)(\\w))$", ["abc", "ab"]], ["(?<=(?:a|ab)(c))d", ["acd", "abcd"]],
  ["(?<=\\$)\\d+(\\.\\d\\d)?", ["cost $12.50", "$7", "12"]], ["(?<!\\d)\\d{2}(?!\\d)", ["123 45 6", "12"]],
  ["(?<=x{2,3})y", ["xy", "xxy", "xxxy", "xxxxy"]], ["(?<=(?<a>.)(?<b>.))c", ["abc"]], ["(?<=\\1(.))c", ["aac", "abc"]],
]) for (const f of ["", "u", "i", "g", "d"]) for (const s of ss) exec(p, f, s);

// ---- 2. Reinício de capturas em cada iteração de um laço.
for (const [p, ss] of [
  ["(?:(a)|b)+", ["ab", "ba", "aba", "bb"]], ["(?:(a)|(b))+", ["ab", "ba"]], ["(?:(a)|(b)){2}", ["ab", "ba", "aa"]],
  ["(?:(a)?b)+", ["abb", "bab", "bb"]], ["(a)?(?:b|\\1c)+", ["abc", "bc"]], ["(?:(\\d)|[a-z])*", ["1a2b"]],
  ["(?:(?<x>a)|(?<y>b))*", ["ab", "ba", ""]], ["(a*)*", ["aaa", "b", ""]], ["(a*)+", ["aaa", "b", ""]], ["(a|ab)(c|bcd)(d*)", ["abcd"]],
  ["(?:(a)|b)*?c", ["abc", "bac"]], ["(z)((a+)?(b+)?(c))*", ["zaacbbbcac"]], ["(?:^(a))?b", ["ab", "b"]], ["(?:(a)){0}b", ["b"]],
  ["(?:(a)){0,1}b", ["ab", "b"]], ["((a)|(b)){2}", ["ab", "ba"]], ["(?=(a))a", ["a"]], ["(?!(a))b", ["b"]], ["(?:(?=(a))a)+", ["aa"]],
  ["(a)|b", ["b"]], ["(?:a|(b))+?c", ["abc"]], ["((a)|b)+", ["ab"]], ["((a)|b)+", ["ba"]],
]) for (const f of ["", "d"]) for (const s of ss) exec(p, f, s);

// ---- 3. Laços que casam vazio (verificação de vazio e limites).
for (const [p, ss] of [
  ["(?:)*", ["a"]], ["(?:a?)*", ["aab"]], ["(?:a?)+", ["aab"]], ["(?:a?){3}", ["aaaa", ""]], ["(?:a|)+", ["aab"]], ["(?:|a)+", ["aab"]],
  ["(?:|a)*", ["aab"]], ["(?:|a)+?", ["aab"]], ["(?:a*)*b", ["aaab", "c"]], ["(?:a*?)*b", ["aaab"]], ["(?:a*){2,3}", ["aaa"]],
  ["(a*){2}", ["aaa"]], ["(a*?){2}", ["aaa"]], ["(?:\\b)*x", ["x"]], ["(?:^)*a", ["a"]], ["(?:$)+", ["a"]], ["(?=a)*a", ["a"]],
  ["(?:(?=a))+a", ["a"]], ["(?:x*)+?y", ["xxy"]], ["(?:a{0})+b", ["b"]], ["(?:a{0,0})*b", ["b"]], ["(?:){2,5}", ["a"]],
  ["((?:)|a)+", ["aa"]], ["(a?)*?b", ["aab"]], ["(a?)*?$", ["aa"]], ["(?:a??)*", ["aaa"]], ["(?:a??)+b", ["aaab"]],
]) for (const f of ["", "g", "y"]) for (const s of ss) exec(p, f, s);

// ---- 4. Case folding de caracteres especiais: i, iu, iv.
const specials = ["s", "S", "ſ", "k", "K", "K", "ß", "ẞ", "İ", "ı", "i", "I", "σ", "ς", "Σ",
  "ǅ", "Ǆ", "ǆ", "µ", "μ", "Μ", "å", "Å", "Å", "ͅ", "ι", "ι", "ﬀ", "ff", "Ω", "ω"];
for (const f of ["i", "iu", "iv"]) {
  for (const a of specials) for (const b of specials) {
    if (a === b) continue;
    add(`T(()=>${re(a, f)}.test(${q(b)}))`);
  }
  for (const c of ["ſ", "K", "K", "s", "ß", "İ"]) {
    add(`T(()=>${re("\\w", f)}.test(${q(c)}))`, `T(()=>${re("\\W", f)}.test(${q(c)}))`, `T(()=>${re("[^\\W]", f)}.test(${q(c)}))`,
      `T(()=>${re("\\b", f)}.test(${q(c)}))`, `T(()=>${re("[\\w-a]", f)}.test(${q(c)}))`, `T(()=>${re("[a-z]", f)}.test(${q(c)}))`,
      `T(()=>${re("[^a-z]", f)}.test(${q(c)}))`, `T(()=>${re("[A-Z]", f)}.test(${q(c)}))`);
  }
}
for (const f of ["iu", "iv"]) for (const p of ["\\p{Lu}", "\\p{Ll}", "\\P{Lu}", "\\P{Ll}", "[^\\p{Lu}]", "[^\\P{Lu}]", "\\p{Lt}", "\\p{Lowercase}", "\\p{Uppercase}"]) {
  for (const c of ["a", "A", "ǅ", "ß", "K", "1", "ς"]) add(`T(()=>${re(p, f)}.test(${q(c)}))`);
}

// ---- 5. Coerção de lastIndex em exec, test, sticky e global.
const lastIndexes = ["0", "1", "2", "3", "-1", "1.9", "'1'", "'x'", "NaN", "Infinity", "-Infinity", "2**53", "2**32", "undefined", "null", "true",
  "{valueOf(){return 1}}", "{valueOf(){return {}}}", "1n"];
for (const f of ["", "g", "y", "gy"]) for (const li of lastIndexes) {
  add(`T(()=>{var r=${re("a", f)};r.lastIndex=${li};var m=r.exec("aaa");return M(m)+" "+S(r.lastIndex)})`,
    `T(()=>{var r=${re("a*", f)};r.lastIndex=${li};var m=r.test("baa");return m+" "+S(r.lastIndex)})`);
}
add(`T(()=>{var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return r.exec("a")})`,
  `T(()=>{var r=/a/;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("a"))})`,
  `T(()=>{var r=/b/;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("a"))})`,
  `T(()=>{var r=/b/g;Object.defineProperty(r,"lastIndex",{writable:false,value:0});return M(r.exec("a"))})`,
  `T(()=>{var r=/a/y;Object.defineProperty(r,"lastIndex",{writable:false,value:5});return M(r.exec("a"))})`,
  `T(()=>Object.getOwnPropertyDescriptor(/a/,"lastIndex").writable)`, `T(()=>Object.getOwnPropertyNames(/a/).join())`);

// ---- 6. Backtracking pesado com limites pequenos (terminam rápido).
for (const n of [1, 2, 5, 8, 12, 16, 20]) {
  const a = "a".repeat(n);
  for (const p of ["(a+)+b", "(a*)*b", "(a|aa)+b", "(?:a+)+?b", "(a+?)+?b", "^(a+)+$", "(a?){5}a{5}", "(?:a|a)+c", "(a|b|ab)*c", "(.*)*x", "(.*?)*?x", "(?:a?){8}a{8}", "a*a*a*b", "(?:\\w+\\s?)+$", "([ab]+)+\\1c"]) {
    add(`T(()=>${p.startsWith("(?:\\w") ? re(p, "") + ".test(" + q(a + "!") + ")" : re(p, "") + ".test(" + q(a) + ")"})`);
  }
  add(`T(()=>M(${re("^(a+)\\1*,\\1+$", "")}.exec(${q(a + "," + a)})))`, `T(()=>M(${re("(a+?)\\1+?$", "")}.exec(${q(a)})))`,
    `T(()=>M(${re("^(?:(a)|b)*?\\1{2,}$", "")}.exec(${q(a)})))`, `T(()=>M(${re("(?:a{1,3}){2,4}b?", "")}.exec(${q(a)})))`);
}
for (const [p, s] of [["^(?:a|ab|abc|abcd)+$", "abcdabcabab"], ["^(\\d+)*$", "123456789a"], ["(x+x+)+y", "xxxxxxxxxxz"], ["^(([a-z])+.)+[A-Z]([a-z])+$", "aaaaaaaaaaaaaaaa!"],
  ["(a|b)*?c", "ababababababc"], ["(a|b)*c", "ababababababc"], ["a.*?b.*?c", "a1b2b3c"], ["a.*b.*c", "a1b2b3c"], ["(?=(a+))a*b\\1", "baaabac"],
  ["(.*)(.*)(.*)(.*)(.*)x", "abcdefgh"], ["^(a{1,3}){1,3}$", "aaaaaaaaa"], ["^(a{1,3}){1,3}$", "aaaaaaaaaa"], ["(?:(?:a{2}){2}){2}", "aaaaaaaaaaaaaaaa"]]) {
  exec(p, "", s);
  add(`T(()=>${re(p, "")}.test(${q(s)}))`);
}

// ---- 7. Sintaxe Annex B (sem u) e seus erros com u/v.
const annex = ["a{", "a{1", "a{1,", "a{,1}", "a{1,2", "{", "}", "]", "a]", "x{1}{2}", "\\c", "\\c1", "\\cA", "\\ca", "[\\c]", "[\\c1]", "[\\cA]", "\\8", "\\9", "\\08", "\\1", "\\01", "\\012", "\\400", "\\377", "\\0",
  "\\00", "(a)\\2", "(a)\\1", "\\k", "\\k<a", "\\k<a>", "(?<a>x)\\k<a>", "(?<a>x)\\k", "\\u", "\\u12", "\\u{1}", "\\u{110000}", "\\x", "\\x1", "\\xg", "\\-", "\\e", "\\a", "\\q", "[\\b]", "[\\B]", "[a-\\d]", "[\\d-a]", "[\\d-\\w]",
  "[a-\\w]", "[\\w-]", "[-a]", "[a-]", "[z-a]", "(?=a)*", "(?=a)+", "(?!a){2}", "(?<=a)*", "(?:a)**", "a**", "a+*", "a?+", "a{1}?", "a{1}+", "^*", "$*", "\\b*", "\\B+", "(?=a){1,2}", "\\p{L}", "\\P{L}", "\\p", "\\p{", "\\pL",
  "(?<=a)", "(?<a>a)(?<a>b)", "(?<a>a)|(?<a>b)", "(?<a", "(?<>a)", "(?<1a>a)", "(?<\\u0061>a)", "(?<a\\u{62}>x)\\k<ab>", "(?", "(?:", "(?a)", "(?i)", "(?i:a)", "(?-i:a)", "(?i-i:a)", "(?ii:a)", "(?-:a)", "(?m-s:a)", "(?x:a)",
  "[", "[]", "[^]", "[]]", "[^]]", "(", ")", "a|", "|a", "()", "(?:)", "a{2,1}", "a{99999999999999999999}", "a{1,99999999999999999999}", "\\", "a\\"];
for (const p of annex) for (const f of ["", "u", "v"]) {
  add(`T(()=>String(${re(p, f)}))`);
  if (f === "") add(`T(()=>M(${re(p, f)}.exec(${q("a{1}a{,1}{x]\\ab\u0001\u0008\u0000\u0001ÿĀabab")})))`);
}

// ---- 8. Symbol.match/replace/search/split/matchAll em subclasses e com exec personalizado.
const execBehaviors = {
  nul: "return null", arr: "var a=['x'];a.index=0;return a", obj: "return {index:1,length:1,0:'y'}", prim: "return 1", thr: "throw new RangeError('boom')",
  once: "if(this.done)return null;this.done=true;var a=['b'];a.index=1;return a", len0: "return {length:0,index:0}", idx: "return {length:1,0:'q',index:'2'}",
  big: "return {length:2,0:'q',1:'r',index:0,groups:{n:'z'}}", empty: "var a=[''];a.index=0;return a",
};
for (const [name, body] of Object.entries(execBehaviors)) for (const fl of ["", "g", "y", "gy"]) {
  const base = `class R2 extends RegExp{exec(s){${body}}};var r=new R2("a",${q(fl)});`;
  add(`T(()=>{${base}return S(r[Symbol.match]("abc"))})`, `T(()=>{${base}return S(r[Symbol.replace]("abc","[$&]"))})`,
    `T(()=>{${base}return S(r[Symbol.search]("abc"))+" "+r.lastIndex})`, `T(()=>{${base}return S(r[Symbol.split]("abc"))})`,
    `T(()=>{${base}return S(r[Symbol.split]("abc",2))})`, `T(()=>{${base}return S("abc".replace(r,"<$1>"))})`,
    `T(()=>{${base}return S(Array.from(r[Symbol.matchAll]("abc"),String))})`, `T(()=>{${base}return r.test("abc")})`);
}
add(`T(()=>{var log=[];class R2 extends RegExp{constructor(p,f){log.push("ctor "+p+" "+f);super(p,f)}};var r=new R2("a","g");log.length=0;r[Symbol.split]("banana");return log.join("|")})`,
  `T(()=>{var log=[];class R2 extends RegExp{static get [Symbol.species](){log.push("species");return RegExp}};var r=new R2("a");r[Symbol.split]("banana");return log.join("|")})`,
  `T(()=>{class R2 extends RegExp{static get [Symbol.species](){return null}};return S(new R2("a")[Symbol.split]("banana"))})`,
  `T(()=>{class R2 extends RegExp{static get [Symbol.species](){return undefined}};return S(new R2("a")[Symbol.split]("banana"))})`,
  `T(()=>{class R2 extends RegExp{static get [Symbol.species](){return 1}};return S(new R2("a")[Symbol.split]("banana"))})`,
  `T(()=>{class R2 extends RegExp{static get [Symbol.species](){return function(p,f){return /n/y}}};return S(new R2("a")[Symbol.split]("banana"))})`,
  `T(()=>{class R2 extends RegExp{static get [Symbol.species](){return function(p,f){return {exec(){return null},lastIndex:0,flags:f}}}};return S(new R2("a")[Symbol.split]("banana"))})`,
  `T(()=>{var log=[];var r=/a/g;Object.defineProperty(r,"flags",{get(){log.push("flags");return "gy"}});r[Symbol.split]("banana");return log.join()})`,
  `T(()=>{var log=[];var r=/a/g;Object.defineProperty(r,"global",{get(){log.push("global");return true}});Object.defineProperty(r,"unicode",{get(){log.push("unicode");return false}});r[Symbol.match]("banana");return log.join()})`,
  `T(()=>{var log=[];var r=/a/;Object.defineProperty(r,"flags",{get(){log.push("flags");return "g"}});r[Symbol.match]("banana");return log.join()})`,
  `T(()=>{var log=[];var r=/a/;Object.defineProperty(r,"flags",{get(){log.push("flags");return "g"}});r[Symbol.replace]("banana","x");return log.join()})`,
  `T(()=>{var log=[];var r=/a/;Object.defineProperty(r,"flags",{get(){log.push("flags");return "u"}});Object.defineProperty(r,"global",{get(){log.push("global");return true}});r[Symbol.matchAll]("banana");return log.join()})`,
  `T(()=>{var r=/a/;Object.defineProperty(r,"flags",{value:"g"});return S("banana".match(r))})`,
  `T(()=>{var r=/a/g;Object.defineProperty(r,"flags",{value:""});return S("banana".match(r))})`,
  `T(()=>{var r=/a/g;Object.defineProperty(r,"flags",{value:"g"});return S("banana".matchAll(r).next().value)})`,
  `T(()=>{var r=/a/;Object.defineProperty(r,"flags",{value:"g"});return S([..."banana".matchAll(r)].length)})`,
  `T(()=>{var r=/a/g;r.lastIndex=3;var it=r[Symbol.matchAll]("banana");r.lastIndex=0;return S([...it].map(m=>m.index))})`,
  `T(()=>{var r=/a/g;Object.defineProperty(r,"lastIndex",{writable:false});return S("banana".replace(r,"x"))})`,
  `T(()=>{var r=/a/y;r.lastIndex=1;return S(r[Symbol.search]("banana"))+r.lastIndex})`,
  `T(()=>{var r=/a/y;r.lastIndex=5;return S(r[Symbol.search]("banana"))+r.lastIndex})`,
  `T(()=>{var r=/a/;r.lastIndex=3;return S(r[Symbol.search]("banana"))+r.lastIndex})`,
  `T(()=>{var r=/x/;r.lastIndex=3;return S(r[Symbol.search]("banana"))+r.lastIndex})`,
  `T(()=>{var r=/a/;Object.defineProperty(r,"lastIndex",{writable:false,value:3});return S(r[Symbol.search]("banana"))})`,
  `T(()=>RegExp.prototype[Symbol.match].call({},"a"))`, `T(()=>RegExp.prototype[Symbol.match].call({exec(){return null},flags:""},"a"))`,
  `T(()=>RegExp.prototype[Symbol.replace].call({exec(){return null},flags:""},"a","b"))`, `T(()=>RegExp.prototype[Symbol.search].call({exec(){return null},lastIndex:0},"a"))`,
  `T(()=>RegExp.prototype[Symbol.split].call({constructor:undefined,flags:"",exec(){return null}},"a"))`, `T(()=>RegExp.prototype[Symbol.matchAll].call({},"a"))`,
  `T(()=>RegExp.prototype[Symbol.match].call(1,"a"))`, `T(()=>RegExp.prototype[Symbol.match].call(undefined,"a"))`, `T(()=>RegExp.prototype.exec.call({},"a"))`,
  `T(()=>RegExp.prototype.test.call({exec(){return {}}},"a"))`, `T(()=>RegExp.prototype.test.call({exec(){return 1}},"a"))`, `T(()=>RegExp.prototype.test.call({exec(){return null}},"a"))`,
  `T(()=>RegExp.prototype.test.call(1,"a"))`, `T(()=>RegExp.prototype.toString.call({source:"a",flags:"b"}))`, `T(()=>RegExp.prototype.toString.call({}))`, `T(()=>RegExp.prototype.toString.call(1))`,
  `T(()=>RegExp.prototype.compile.call({},"a"))`, `T(()=>/a/.compile("b","g").flags)`, `T(()=>/a/.compile(/b/i).flags)`, `T(()=>/a/.compile(/b/i,"g"))`, `T(()=>/a/.compile(undefined).source)`, `T(()=>/a/g.compile("b").lastIndex)`,
  `T(()=>{class R2 extends RegExp{};return new R2("a").compile("b").source})`, `T(()=>{class R2 extends RegExp{};return RegExp.prototype.compile.call(new R2("a"),"b").source})`);

// ---- 9. Descritores e getters do protótipo.
for (const k of ["flags", "source", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky", "hasIndices", "lastIndex", "exec", "test", "toString", "compile"]) {
  add(`T(()=>{var d=Object.getOwnPropertyDescriptor(RegExp.prototype,${q(k)});return d?Object.keys(d).join()+" "+d.enumerable+d.configurable+(d.get?d.get.name:d.value&&d.value.name)+(d.get?d.get.length:d.value&&d.value.length):"none"})`);
  if (!["lastIndex", "exec", "test", "toString", "compile"].includes(k)) {
    add(`T(()=>RegExp.prototype[${q(k)}])`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,${q(k)}).get.call({}))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,${q(k)}).get.call(1))`,
      `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,${q(k)}).get.call(/a/gimsuyd))`, `T(()=>Object.getOwnPropertyDescriptor(RegExp.prototype,${q(k)}).get.call(/a/v))`);
  }
}
add(`T(()=>String(RegExp.prototype))`, `T(()=>RegExp.prototype.source)`, `T(()=>RegExp.prototype.flags)`, `T(()=>Object.prototype.toString.call(RegExp.prototype))`, `T(()=>RegExp.length+RegExp.name)`,
  `T(()=>Object.getOwnPropertyNames(RegExp).sort().join())`, `T(()=>Reflect.ownKeys(RegExp.prototype).map(String).sort().join())`, `T(()=>RegExp.prototype.constructor===RegExp)`,
  `T(()=>Object.getOwnPropertyDescriptor(RegExp,Symbol.species).get.name)`, `T(()=>RegExp[Symbol.species]===RegExp)`, `T(()=>RegExp.prototype[Symbol.split].name)`, `T(()=>RegExp.prototype[Symbol.matchAll].length)`,
  `T(()=>new RegExp("a","gimsuyd").flags)`, `T(()=>new RegExp("a","dgimsvy").flags)`, `T(()=>new RegExp("a","yvsmigd").flags)`, `T(()=>new RegExp("a","uv"))`, `T(()=>new RegExp("a","gg"))`, `T(()=>new RegExp("a","x"))`,
  `T(()=>new RegExp("a",{toString(){return "g"}}).flags)`, `T(()=>new RegExp("a",null))`, `T(()=>new RegExp("a",undefined).flags)`, `T(()=>new RegExp(undefined).source)`, `T(()=>new RegExp(null).source)`, `T(()=>new RegExp().source)`,
  `T(()=>RegExp(/a/g)===undefined)`, `T(()=>{var r=/a/g;return RegExp(r)===r})`, `T(()=>{var r=/a/g;return RegExp(r,"i")===r})`, `T(()=>{var r=/a/g;return new RegExp(r)===r})`, `T(()=>{var r=/a/g;r.constructor=1;return RegExp(r)===r})`,
  `T(()=>{var r=/a/g;r[Symbol.match]=false;return RegExp(r)===r})`, `T(()=>{var o={source:"x",flags:"i",constructor:RegExp,[Symbol.match]:true};return RegExp(o)===o})`, `T(()=>{var o={source:"x",flags:"i",[Symbol.match]:true};return String(new RegExp(o))})`,
  `T(()=>{var o={source:"x",[Symbol.match]:true};return String(new RegExp(o))})`, `T(()=>{var o={flags:"g",[Symbol.match]:true};return String(new RegExp(o))})`, `T(()=>String(new RegExp(/a/g,"i")))`, `T(()=>String(new RegExp(/a\\/b/g)))`,
  `T(()=>String(new RegExp(/a/g,undefined)))`, `T(()=>String(new RegExp(/a/g,"")))`);

// ---- 10. source e toString: escapes, novas linhas, barras.
const srcs = ["/", "\\/", "[/]", "[\\/]", "a/b", "\n", "\r", " ", " ", "\\n", "\\\n", "[\n]", "\\\\", "\\\\/", "a\\", "[", "(?:)", "", "/*", "*", "\t", "\u0000", "\\0", "[\\]/]", "(?<n>/)", "\\u002f", "\\x2F", "\\/\\/", "//", "\\\\\\/", "[^/]", "[]/]"];
for (const s of srcs) for (const f of ["", "u", "v"]) {
  add(`T(()=>${re(s, f)}.source)`, `T(()=>String(${re(s, f)}))`);
}
for (const s of srcs.slice(0, 20)) add(`T(()=>{var r=${re(s, "")};return new RegExp(r.source).source===r.source})`, `T(()=>{var r=${re(s, "g")};return String(new RegExp(String(r).slice(1,String(r).lastIndexOf("/")),"g"))===String(r)})`, `T(()=>RegExp.prototype.toString.call({source:${q(s)},flags:"z"}))`);
add(`T(()=>/\\//.source)`, `T(()=>/[/]/.source)`, `T(()=>/a\\\nb/.source)`, `T(()=>eval("/[\\/]/").source)`, `T(()=>eval("/\\u{1F600}/u").source)`, `T(()=>eval("/\\//").toString())`, `T(()=>eval("/(?:)/").source)`, `T(()=>new RegExp("\\u2028").toString().length)`);

// ---- 11. Propriedades Unicode em amostras.
const props = ["L", "Lu", "Ll", "Lt", "Lm", "Lo", "LC", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "S", "Sm", "Sc", "Sk", "So", "Z", "Zs", "Zl", "Zp", "C", "Cc", "Cf", "Cs", "Co", "Cn",
  "Letter", "Uppercase_Letter", "Decimal_Number", "Punctuation", "punct", "digit", "Combining_Mark", "Other",
  "Script=Latin", "sc=Grek", "Script=Cyrillic", "Script=Han", "Script=Hiragana", "Script=Katakana", "Script=Arabic", "Script=Hebrew", "Script=Common", "Script=Inherited", "Script=Thai", "Script=Devanagari", "Script=Greek", "Script=Zyyy", "Script=Zinh",
  "Script_Extensions=Latin", "scx=Grek", "scx=Hira", "scx=Kana", "scx=Deva", "scx=Beng", "scx=Arab", "scx=Han", "scx=Common", "scx=Cyrl", "Script_Extensions=Hebrew",
  "Alphabetic", "ASCII", "ASCII_Hex_Digit", "AHex", "Any", "Assigned", "Emoji", "Emoji_Presentation", "Emoji_Modifier", "Emoji_Modifier_Base", "Emoji_Component", "Extended_Pictographic", "ID_Start", "ID_Continue", "XID_Start", "XID_Continue",
  "Lowercase", "Uppercase", "White_Space", "Hex_Digit", "Ideographic", "Math", "Dash", "Diacritic", "Extender", "Case_Ignorable", "Cased", "Changes_When_Lowercased", "Changes_When_Uppercased", "Changes_When_Casefolded", "Changes_When_NFKC_Casefolded",
  "Default_Ignorable_Code_Point", "Grapheme_Base", "Grapheme_Extend", "Join_Control", "Noncharacter_Code_Point", "Pattern_Syntax", "Pattern_White_Space", "Quotation_Mark", "Regional_Indicator", "Sentence_Terminal", "Terminal_Punctuation", "Unified_Ideograph", "Variation_Selector", "Bidi_Control", "Bidi_Mirrored", "Deprecated", "IDS_Binary_Operator", "Logical_Order_Exception", "Radical", "Soft_Dotted", "Lowercase_Letter"];
const samples = ["a", "A", "ǅ", "ʰ", "ª", "́", "ः", "⃝", "5", "٣", "Ⅰ", "½", "_", "-", "(", ")", "«", "»", "!", "+", "$", "^", "©", " ", " ", " ", "\u0000", "­", "\ud800".length ? "\u{e000}" : "", "͸",
  "α", "а", "中", "あ", "ア", "ا", "א", "ก", "क", "ー", "、", "॑", "\u{1f600}", "\u{1f3fb}", "‍", "\u{1f1e6}", "#", " ", "K", "ı", "\u{10400}", "\u{1d7d8}", "\u{e0001}", "\u{fffe}", "\u{10ffff}", "️", "❤", "µ"];
for (const p of props) {
  add(`T(()=>{var r=${re("\\p{" + p + "}", "u")};return ${q(samples.join(""))}.split("").filter(c=>r.test(c)).length+" "+[...${q(samples.join(""))}].filter(c=>r.test(c)).map(c=>c.codePointAt(0).toString(16)).join()})`);
  add(`T(()=>{var r=${re("\\P{" + p + "}", "v")};return [...${q(samples.join(""))}].filter(c=>r.test(c)).map(c=>c.codePointAt(0).toString(16)).join()})`);
  add(`T(()=>{var r=${re("[^\\p{" + p + "}]", "ui")};return [...${q(samples.join(""))}].filter(c=>r.test(c)).map(c=>c.codePointAt(0).toString(16)).join()})`);
}
for (const bad of ["\\p{}", "\\p{Foo}", "\\p{Script=}", "\\p{Script=Foo}", "\\p{Script_Extensions=Foo}", "\\p{General_Category}", "\\p{Lu=}", "\\p{lu}", "\\p{ L }", "\\p{Script = Latin}", "\\p{Block=Basic_Latin}", "\\p{InBasicLatin}", "\\p{IsLatin}", "\\p{Age=1.1}", "\\p{Alphabetic=Yes}", "\\p{Any=Y}",
  "\\p{ASCII", "\\P", "\\p{Emoji_Keycap_Sequence}", "\\p{RGI_Emoji}", "\\P{RGI_Emoji}", "[^\\p{RGI_Emoji}]", "\\p{Basic_Emoji}", "\\p{General_Category=Lu}", "\\p{gc=Lu}", "\\p{gc=L&}", "\\p{LC}", "\\p{L&}", "\\p{sc=Latn}", "\\p{scx=Latn}", "\\p{Script=latin}", "\\p{Lowercase=true}"])
  for (const f of ["u", "v"]) add(`T(()=>String(${re(bad, f)}))`);

// ---- 12. Flag d (indices).
for (const [p, ss] of [
  ["(a)(b)?", ["a", "ab"]], ["(?<x>a)(?<y>b)?", ["a", "ab", "zab"]], ["(?:(a)|(b))+", ["ab"]], ["(?<=(a))b", ["ab"]], ["(?=(a))a", ["a"]], ["(a)\\1", ["aa"]], ["(?<x>a)|(?<x>b)", ["b", "a"]],
  ["\\u{1F600}(.)", ["\u{1F600}x"]], ["(.)", ["\u{1F600}"]], ["(\\p{L})+", ["abé"]], ["()", [""]], ["(a*)", ["baa"]], ["((a)(b))", ["ab"]], ["(?<a>.)(?<b>.)(?<c>.)", ["xyz"]],
]) for (const f of ["d", "dg", "du", "dy", "dgu", "dv", "di", "dm", "ds"]) for (const s of ss) exec(p, f, s);
add(`T(()=>{var m=/(?<a>x)/d.exec("x");return Object.getOwnPropertyNames(m).join()+" "+Object.getOwnPropertyNames(m.indices).join()+" "+Object.getPrototypeOf(m.indices.groups)})`,
  `T(()=>{var m=/(x)/d.exec("x");return Object.getOwnPropertyNames(m).join()+" "+Object.getOwnPropertyNames(m.indices).join()+" "+m.indices.groups})`,
  `T(()=>{var m=/(x)/.exec("x");return Object.getOwnPropertyNames(m).join()+" "+("indices" in m)})`,
  `T(()=>{var m=/(?<a>x)/.exec("x");return Object.getPrototypeOf(m.groups)+" "+Object.getOwnPropertyNames(m).join()})`,
  `T(()=>{var m=/(?<a>x)/d.exec("x");return Object.isFrozen(m.indices)+" "+Object.isExtensible(m.groups)+" "+Array.isArray(m.indices)+" "+Array.isArray(m.indices[0])})`,
  `T(()=>{var m=/(?<a>x)/d.exec("x");return JSON.stringify(Object.getOwnPropertyDescriptor(m,"indices"))+JSON.stringify(Object.getOwnPropertyDescriptor(m,"groups"))+JSON.stringify(Object.getOwnPropertyDescriptor(m,"index"))+JSON.stringify(Object.getOwnPropertyDescriptor(m,"input"))})`,
  `T(()=>{var r=/a/d;r.lastIndex=1;return M(r.exec("aa"))+r.lastIndex})`, `T(()=>"aXbX".replace(/(?<n>X)/dg,(...a)=>JSON.stringify(a[a.length-1])))`, `T(()=>[..."aXbX".matchAll(/X/dg)].map(m=>m.indices[0]).join("|"))`);

// ---- 13. Mensagens exatas de SyntaxError (construtor e flags).
const bads = ["(", ")", "(?", "(?<", "(?<n", "(?<n>", "[", "[a-", "[z-a]", "a**", "a{2,1}", "*", "+a", "?", "\\", "(?<a>x)(?<a>y)", "\\k<z>", "(?<a>x)\\k<b>", "\\1(", "(?=a", "(?<=a", "(?:a", "a)", "[\\d-a]", "\\u{110000}", "\\p{Foo}", "\\c", "\\8", "{1}", "a{1}{2}", "(?i-i:a)",
  "(?i:", "(?-:a)", "(?<=a)+", "(?=a)+", "\\u{}", "\\x1", "[\\p{L}-z]", "[a&&&b]", "[(]", "[a--]", "\\q{a}", "[\\q{a}", "(?<𝒜>.)", "(?<a-b>.)", "(?<$>.)\\k<$>", "(?<_>.)", "\\k<$>", "(?<a>.)\\k<a", "\\k<a>"];
for (const p of bads) for (const f of ["", "u", "v", "g", "gimsuyd", "iv", "uv", "gg", "z", "G", "dd", "ii", "yy", "sv"]) add(`T(()=>String(${re(p, f)}))`);
for (const lit of ["/(/", "/a**/", "/[/", "/(?<a>.)(?<a>.)/", "/\\u{110000}/u", "/a/gg", "/a/z", "/(?<=a)+/", "/\\k<a>/u", "/[b-a]/", "/{/u", "/a{2,1}/", "/\\p{Foo}/u", "/(?i:a)/", "/(?i-i:a)/", "/(?ii:a)/", "/(?<a>.)\\k<b>/", "/\\1/u", "/(?:)/uv", "/[a-z&&[aeiou]]/v", "/[a&&&b]/v", "/\\q{a}/v"])
  add(`T(()=>eval(${q(lit)}).source)`);

// ---- 14. matchAll, split e replace com bordas em cadeias com pares substitutos.
for (const p of ["", "(?:)", "a*", "\\b", "$", "^", "(?=a)", "\\B", "[^]", ".", "(?:)|a", "x*"]) for (const f of ["g", "gu", "gv", "gy", "gm"]) {
  for (const s of ["aab", "\u{1F600}a\u{1F600}", "", "a\nb"]) {
    add(`T(()=>S([...${q(s)}.matchAll(${re(p, f)})].map(m=>m.index+":"+m[0])))`, `T(()=>S(${q(s)}.replace(${re(p, f)},"|")))`, `T(()=>S(${q(s)}.split(${re(p, f.replace("g", ""))})))`);
  }
}
for (const [s, lim] of [["a,b,c", 0], ["a,b,c", 1], ["a,b,c", 2], ["a,b,c", 3], ["a,b,c", -1], ["a,b,c", 2 ** 32], ["a,b,c", 2 ** 32 + 1], ["a,b,c", "2"], ["a,b,c", NaN], ["a,b,c", undefined], ["", 5], [",", 5]])
  for (const p of [",", "(,)", "(?:,)", "(x)?,", "", "(?=,)", "$", "^", ",+", "(?<=,)"]) add(`T(()=>S(${q(s)}.split(${re(p, "")},${typeof lim === "number" ? (Number.isNaN(lim) ? "NaN" : lim) : q(lim)})))`);
for (const rep of ["$", "$$", "$&", "$`", "$'", "$1", "$01", "$10", "$001", "$<n>", "$<", "$<m>", "$<n", "$0", "$00", "$2", "$11", "$$1", "$$$", "\\$1", "$ ", "$n"])
  for (const [p, s] of [["(?<n>b)", "abc"], ["(b)", "abc"], ["b", "abc"], ["(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)", "abcdefghijk"]])
    add(`T(()=>S(${q(s)}.replace(${re(p, "")},${q(rep)})))`, `T(()=>S(${q(s)}.replace(${re(p, "g")},${q(rep)})))`, `T(()=>S(${q(s)}.replaceAll(${re(p, "g")},${q(rep)})))`);

// ---- Execução: cada programa num bun filho novo; descarta prelúdio grande e programas que falham.
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + (/^T\(/.test(expr) ? `globalThis.R = ${expr}` : `globalThis.R = T(()=>{return ${expr}})`);
  const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", timeout: 5000, maxBuffer: 1 << 24 });
  const result = child.status === 0 && !child.error ? decodeResult(child.stdout) : null;
  if (result === null) {
    process.stderr.write("falhou: " + JSON.stringify(expr).slice(0, 160) + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result) || result.length > 6000) {
    dropped++;
    continue;
  }
  if (/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(source + result) || /\\ud[89ab][0-9a-f]{2}(?!\\ud[c-f])|(?<!\\ud[89ab][0-9a-f]{2})\\ud[c-f][0-9a-f]{2}/i.test(source + result)) {
    dropped++;
    continue;
  }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
process.stdout.write(emitFactored("regexp_depth", rows));
