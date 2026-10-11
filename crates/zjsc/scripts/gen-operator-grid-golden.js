// Gera tests/golden/operator_grid_bun.tsv: grade de operadores contra valores exóticos, medida no bun 1.4.2.
// Complementa coercion_bun (matriz sem rastro), coercion_semantics_bun e operator_edge_bun (casos escolhidos à mão):
// aqui cada operador binário (== != === !== < > <= >= + - * / % ** << >> >>> & | ^ in instanceof ?? || &&) roda sobre
// pares amostrados de forma determinística (LCG, semente fixa) de uma lista de 70 valores, metade deles objetos que
// registram a ordem das chamadas de valueOf/toString/@@toPrimitive (com o hint) e de armadilhas de Proxy. Também
// cobre os operadores unários, ++/-- em todos os valores, atribuição composta e lógica (||= &&= ??=) sobre
// propriedade com getter/setter que registra, encadeamento opcional, `in`/`instanceof` com @@hasInstance e Proxy,
// e as conversões explícitas (Number, String, BigInt, parseInt, template literal, Object.is).
// Resultado: `ser(valor)|rastro` ou `throw Nome: mensagem|rastro`, com `-0`, BigInt (`n`) e Symbol distintos.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host, e grava `globalThis.R`.
// O prelúdio comum sai em tests/golden/operator_grid.preludes.json e as linhas só levam o sufixo (scripts/golden-prelude.js).
// Uso: bun scripts/gen-operator-grid-golden.js > tests/golden/operator_grid_bun.tsv
const fs = require("fs");
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  "var L=[];\n" +
  "function S(v){var t=typeof v;if(t==='string')return JSON.stringify(v);if(t==='symbol')return 'sym:'+v.description;" +
  "if(t==='function')return 'fn';if(t==='bigint')return v+'n';if(v===null)return 'null';if(t!=='object')return Object.is(v,-0)?'-0':String(v);" +
  "if(Array.isArray(v))return '['+v.map(S).join(',')+']';return 'obj'}\n" +
  "function RUN(f){L=[];var r;try{r=S(f())}catch(e){r='throw '+(e&&e.name)+': '+(e&&e.message)}return r+'|'+L.join()}\n" +
  "function W(n,k){switch(k){" +
  "case 'vo':return {valueOf(){L.push(n+'.v');return 3},toString(){L.push(n+'.s');return 'S'}};" +
  "case 'vs':return {valueOf(){L.push(n+'.v');return {}},toString(){L.push(n+'.s');return '5'}};" +
  "case 'tpn':return {[Symbol.toPrimitive](h){L.push(n+':'+h);return 2n}};" +
  "case 'tps':return {[Symbol.toPrimitive](h){L.push(n+':'+h);return ' 0x10 '}};" +
  "case 'tpy':return {[Symbol.toPrimitive](h){L.push(n+':'+h);return Symbol('r')}};" +
  "case 'thr':return {valueOf(){L.push(n+'.v');throw new TypeError('boom '+n)}};" +
  "case 'tpnull':return {[Symbol.toPrimitive](h){L.push(n+':'+h);return null}};" +
  "case 'tpobj':return {[Symbol.toPrimitive](h){L.push(n+':'+h);return {}}};" +
  "case 'px':return new Proxy({},{get(t,key){L.push(n+'.get:'+String(key));return undefined}});" +
  "case 'pxp':return new Proxy({},{get(t,key){L.push(n+'.get:'+String(key));return key===Symbol.toPrimitive?function(h){L.push(n+':'+h);return 7}:undefined}});" +
  "case 'pxf':return new Proxy(function(){},{get(t,key){L.push(n+'.get:'+String(key));return t[key]}});" +
  "case 'vbig':return {valueOf(){L.push(n+'.v');return 1n}};" +
  "case 'nul':return {valueOf:1,toString:null};" +
  "case 'hi':return {[Symbol.hasInstance](v){L.push(n+'.hi:'+S(v));return v}};" +
  "}}\n" +
  "function GS(init,n){var s=init;return {get p(){L.push(n+'.get');return s},set p(v){L.push(n+'.set:'+S(v));s=v}}}\n";

const plain = [
  "undefined", "null", "true", "false", "0", "-0", "NaN", "1", "-1", "3.5", "Infinity", "2**31", "2**32", "-(2**31)", "1e21", "''", "' 12 '",
  "'0x1F'", "'0b101'", "'0o17'", "' \\n'", "'Infinity'", "'-Infinity'", "'1_0'", "'1e3'", "'abc'", "'9007199254740993'", "'-0'", "'+5'", "'.5'",
  "1n", "0n", "-1n", "2n**64n", "Symbol('s')", "(function f(){})", "(class K{})", "[]", "[5]", "[1,2]", "({})", "Object(1n)", "new String('7')",
  "new Boolean(false)", "Object.create(null)", "(()=>1)", "Object(Symbol('w'))",
];
const logging = ["vo", "vs", "tpn", "tps", "tpy", "thr", "tpnull", "tpobj", "px", "pxp", "pxf", "vbig", "nul", "hi"];
const valueSources = (label) => [...plain, ...logging.map((k) => `W('${label}','${k}')`)];
const valueCount = plain.length + logging.length;

// LCG determinístico.
let seed = 0x2545f491;
const next = (n) => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  return (seed >>> 8) % n;
};

const programs = [];
const seenPrograms = new Set();
const addExpr = (expr) => {
  if (seenPrograms.has(expr)) return;
  seenPrograms.add(expr);
  programs.push(`globalThis.R = RUN(()=>${expr});`);
};
const addBody = (body) => {
  if (seenPrograms.has(body)) return;
  seenPrograms.add(body);
  programs.push(`globalThis.R = RUN(()=>{${body}});`);
};

// ---- 1. Operadores binários sobre pares amostrados (a e b rotulados para o rastro).
const binary = ["==", "!=", "===", "!==", "<", ">", "<=", ">=", "+", "-", "*", "/", "%", "**", "<<", ">>", ">>>", "&", "|", "^", "??", "||", "&&"];
const PAIRS = 125;
for (const op of binary) {
  const used = new Set();
  let guard = 0;
  while (used.size < PAIRS && guard++ < 5000) {
    // 55% dos pares têm ao menos um operando com rastro.
    const pickLog = () => plain.length + next(logging.length);
    let i = next(valueCount);
    let j = next(valueCount);
    const roll = next(100);
    if (roll < 25) i = pickLog();
    else if (roll < 50) j = pickLog();
    else if (roll < 65) { i = pickLog(); j = pickLog(); }
    const key = i * 1000 + j;
    if (used.has(key)) continue;
    used.add(key);
    addExpr(`(${valueSources("a")[i]}) ${op} (${valueSources("b")[j]})`);
  }
}

// ---- 2. `in` e `instanceof` com direita exótica e esquerda variada.
const lhsKeys = ["'a'", "0", "-0", "1n", "Symbol.iterator", "'length'", "({toString(){L.push('k.s');return 'x'}})", "null", "undefined", "1.5", "'1'", "true"];
const inRhs = [
  "[5,6]", "{x:1}", "'str'", "1", "null", "undefined", "Symbol.iterator", "1n", "function(){}", "Object.create({a:1})",
  "new Proxy({a:1},{has(t,k){L.push('has:'+String(k));return k in t}})",
  "new Proxy({},{has(t,k){L.push('has:'+String(k));return 1}})",
  "new Proxy({},{has(t,k){L.push('has:'+String(k));throw new RangeError('has')}})",
  "new Proxy([1,2],{has(t,k){L.push('has:'+String(k));return false}})",
  "Object('s')", "new String('abc')",
];
for (const key of lhsKeys) for (const rhs of inRhs) addExpr(`(${key}) in (${rhs})`);
const instLhs = ["({})", "[]", "1", "'s'", "null", "undefined", "Symbol('q')", "1n", "(function(){})", "Object.create(null)", "new (class A{})"];
const instRhs = [
  "Object", "Array", "Function", "Symbol", "BigInt", "Math.max", "(()=>1)", "(class{})", "function(){}.bind()", "{}", "1", "null", "undefined", "'s'", "Symbol('r')",
  "W('r','hi')", "{[Symbol.hasInstance]:1}", "{[Symbol.hasInstance]:null}", "{[Symbol.hasInstance]:{}}",
  "{[Symbol.hasInstance](){L.push('hi');return '';}}", "{[Symbol.hasInstance](){L.push('hi');return 'x';}}",
  "{get [Symbol.hasInstance](){L.push('get');return ()=>true}}",
  "(()=>{function F(){};F.prototype=1;return F})()", "(()=>{function F(){};F.prototype=null;return F})()",
  "(()=>{function F(){};F.prototype=Object.create(null);return F})()", "W('r','pxf')", "new Proxy({},{get(t,k){L.push('get:'+String(k));}})",
  "(()=>{var f=function(){};Object.defineProperty(f,Symbol.hasInstance,{value:undefined});return f})()",
  "(()=>{class C{static [Symbol.hasInstance](v){L.push('C.hi:'+S(v));return this===C}};return C})()",
];
for (const lhs of instLhs) for (const rhs of instRhs) addExpr(`(${lhs}) instanceof (${rhs})`);

// ---- 3. Operadores unários e ++/-- em todos os valores (com e sem rastro).
const allValues = valueSources("a");
for (const v of allValues) {
  for (const u of ["+", "-", "~", "!", "typeof ", "void ", "!!"]) addExpr(`${u}(${v})`);
  addBody(`var x=(${v});var r=x++;return [r,x]`);
  addBody(`var x=(${v});var r=++x;return [r,x]`);
  addBody(`var x=(${v});var r=x--;return [r,x]`);
  addBody(`var x=(${v});var r=--x;return [r,x]`);
  addBody(`var o={p:(${v})};var r=o.p++;return [r,typeof o.p]`);
  addBody(`var o={p:(${v})};var r=--o.p;return [r,typeof o.p]`);
}

// ---- 4. Atribuição composta e lógica sobre propriedade com getter/setter que registra.
const assignOps = ["+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "||=", "&&=", "??="];
const inits = ["undefined", "null", "0", "-0", "1", "NaN", "''", "'s'", "' 7 '", "1n", "0n", "true", "Symbol('i')", "({})", "[]", "W('i','vo')", "W('i','tpn')", "W('i','thr')"];
const rhsVals = ["1", "2n", "'x'", "null", "undefined", "NaN", "-1", "W('y','vo')", "W('y','tps')", "W('y','tpy')", "(L.push('rhs'),5)", "Symbol('z')", "3.5", "'0b11'"];
for (const op of assignOps) {
  const used = new Set();
  let guard = 0;
  while (used.size < 28 && guard++ < 1000) {
    const i = next(inits.length);
    const j = next(rhsVals.length);
    if (used.has(i * 100 + j)) continue;
    used.add(i * 100 + j);
    addBody(`var o=GS(${inits[i]},'o');var r=(o.p ${op} ${rhsVals[j]});return [r,L.length]`);
  }
}
for (const op of assignOps) {
  for (const i of [0, 2, 6, 9, 15]) {
    addBody(`var a=[${inits[i]}];var r=(a[L.push('idx'),0] ${op} (L.push('rhs'),4));return [r,a[0]]`);
    addBody(`var x=(${inits[i]});var r=(x ${op} (L.push('rhs'),4));return [r,x]`);
  }
}
addBody("var o=new Proxy({p:1},{get(t,k){L.push('get:'+String(k));return t[k]},set(t,k,v){L.push('set:'+String(k)+'='+S(v));t[k]=v;return true}});var r=(o.p ||= 9);return r");
addBody("var o=new Proxy({p:0},{get(t,k){L.push('get:'+String(k));return t[k]},set(t,k,v){L.push('set:'+String(k)+'='+S(v));t[k]=v;return true}});var r=(o.p ||= 9);return r");
addBody("var o=new Proxy({p:null},{get(t,k){L.push('get:'+String(k));return t[k]},set(t,k,v){L.push('set:'+String(k)+'='+S(v));t[k]=v;return true}});var r=(o.p ??= 9);return r");
addBody("var o=new Proxy({p:2},{get(t,k){L.push('get:'+String(k));return t[k]},set(t,k,v){L.push('set:'+String(k)+'='+S(v));t[k]=v;return true}});var r=(o.p &&= 9);return r");
addBody("var o=new Proxy({p:2},{get(t,k){L.push('get:'+String(k));return t[k]},set(t,k,v){L.push('set:'+String(k)+'='+S(v));t[k]=v;return true}});var r=(o.p **= 2);return r");
addBody("var o=Object.freeze({p:0});var r=(o.p ||= 1);return r");
addBody("var o=Object.freeze({p:1});var r=(o.p ||= 2);return r");
addBody("var o=Object.freeze({p:1});var r=(o.p &&= 2);return r");
addBody("var o=Object.freeze({p:null});var r=(o.p ??= 2);return r");
addBody("var o=Object.freeze({p:1});var r=(o.p ??= 2);return r");
addBody("var o=Object.freeze({p:1});var r=(o.p += 2);return r");
addBody("var o={get p(){L.push('get');return 0}};var r=(o.p ||= 3);return r");
addBody("var o={get p(){L.push('get');return 1}};var r=(o.p &&= 3);return r");
addBody("var o={set p(v){L.push('set')}};var r=(o.p ??= 3);return [r,L.length]");
addBody("var u;var r=(u.p ||= 1);return r");
addBody("var u=null;var r=(u.p ??= 1);return r");
addBody("var u=null;var r=(u[(L.push('key'),'p')] &&= 1);return r");
addBody("var f=function(){};var r=(f.name ||= 'z');return r");
addBody("var r=(undeclared ||= 1);return r");
addBody("var r=(undeclared ??= 1);return r");
addBody("var r=(undeclared += 1);return r");

// ---- 5. Encadeamento opcional e ??, ||, && com curto-circuito.
const optBases = ["null", "undefined", "0", "''", "false", "NaN", "0n", "1", "'s'", "Symbol('s')", "[]", "({})", "(function(){})", "W('a','px')", "W('a','pxp')", "W('a','pxf')", "W('a','vo')", "Object.create(null)", "document_missing()"];
for (const b of optBases) {
  addExpr(`(${b})?.a`);
  addExpr(`(${b})?.["a"]`);
  addExpr(`(${b})?.a.b.c`);
  addExpr(`(${b})?.a?.b`);
  addExpr(`(${b})?.()`);
  addExpr(`(${b})?.a()`);
  addExpr(`(${b})?.[(L.push('key'),'k')]`);
  addExpr(`delete (${b})?.a`);
  addExpr(`typeof (${b})?.a`);
  addExpr(`((${b})?.a).b`);
  addExpr(`(${b}) ?? (L.push('rhs'),1)`);
  addExpr(`(${b}) || (L.push('rhs'),1)`);
  addExpr(`(${b}) && (L.push('rhs'),1)`);
  addExpr(`(${b}) ? 'y' : 'n'`);
  addExpr(`[...[(${b})?.length]]`);
}
for (const e of [
  "null?.[L.push('k')]", "undefined?.a[L.push('k')]", "({a:{b(){L.push('b');return this===undefined?'u':'o'}}}).a?.b()", "({a:null}).a?.b.c.d",
  "({a:{b:null}}).a.b?.c", "({f(){return this}}).f?.()===undefined", "(0,{f(){return this}}.f)?.()===undefined", "({f:null}).f?.()", "({f:1}).f?.()",
  "({}).f?.()", "({}).f()", "({a:1}).a?.()", "[1,2]?.[1]", "'abc'?.[1]", "'abc'?.length", "(1)?.toFixed(1)", "(null)?.toFixed(1)", "(void 0)?.x", "(L.push('a'),null)?.[(L.push('b'),1)]",
  "null ?? undefined ?? 0 ?? 1", "(null ?? undefined) ?? 0", "null ?? (0 || 1)", "0 || null ?? 1", "(0 || null) ?? 1", "(0 ?? 1) || 2", "(null && 1) ?? 2", "1 && null ?? 2",
  "(false ?? 1) + 1", "undefined?.a ?? 'd'", "null?.a?.b ?? 'd'", "(0 && L.push('x')) ?? 1", "(1 && 0) || (L.push('x'),2)", "(1 || L.push('x')) && (L.push('y'),3)",
  "a?.b", "a?.b.c", "globalThis.nothing?.x", "globalThis?.Math?.PI", "Math?.max?.(1,2)", "Math.nothing?.(1)", "Math?.['max'](1,2)",
  "new (class{constructor(){this.a=1}})?.a", "((a)=>a?.b)(null)", "((a)=>a?.b)({b:7})", "(async()=>1)?.()?.constructor?.name",
  "`${null?.a}`", "`${null??'d'}`", "`${0||'d'}`", "!null?.a", "-null?.a", "+undefined?.a", "(null?.a)++===undefined",
]) addExpr(e);

// ---- 6. Conversões explícitas e implícitas em todos os valores.
for (const v of allValues) {
  for (const f of [
    "Number((V))", "String((V))", "Boolean((V))", "BigInt((V))", "parseInt((V))", "parseFloat((V))", "isNaN((V))", "Object.is((V),(V))",
    "`${(V)}`", "(V)+''", "''+(V)", "[(V)]+''", "[(V)].join()", "Math.abs((V))", "Math.max((V),0)", "Number.isInteger((V))", "BigInt.asUintN(8,(V))",
    "({[(V)]:1})", "[1,2,3][(V)]", "'abc'[(V)]", "'x'.repeat((V))", "[0,1,2].at((V))", "new Array((V)).length", "(V)==(V)", "(V)=='1'", "(V)==1n", "(V)<1n", "(V)<'1'", "'a'<(V)",
    "Symbol.keyFor((V))", "String((V)).length", "JSON.stringify((V))", "isFinite((V))", "Number.parseFloat((V))", "(V)|0", "(V)>>>0", "2**(V)", "(V)%2", "typeof ((V)*1)",
  ]) addExpr(f.split("(V)").join(`(${v})`));
}

// Conversões com rastro entre operandos da mesma expressão (ordem de ToPrimitive nos dois lados).
for (const kinds of [["vo", "vs"], ["tpn", "tps"], ["vo", "thr"], ["thr", "vo"], ["tpobj", "vo"], ["vo", "tpnull"], ["px", "pxp"], ["pxp", "pxf"], ["vbig", "tpn"], ["tps", "tpn"], ["nul", "vo"], ["vo", "nul"], ["tpy", "vo"], ["vo", "tpy"]]) {
  for (const op of ["+", "-", "*", "<", ">", "<=", ">=", "==", "**", "<<", "&", "in", "instanceof"]) {
    addExpr(`(W('a','${kinds[0]}')) ${op} (W('b','${kinds[1]}'))`);
  }
  addExpr(`[W('a','${kinds[0]}'),W('b','${kinds[1]}')]+''`);
  addExpr(`\`\${W('a','${kinds[0]}')}-\${W('b','${kinds[1]}')}\``);
  addExpr(`(L.push('x'),W('a','${kinds[0]}'))+(L.push('y'),W('b','${kinds[1]}'))`);
  addExpr(`({[W('a','${kinds[0]}')]:W('b','${kinds[1]}')})`);
  addExpr(`[1,2,3][W('a','${kinds[0]}')]`);
  addExpr(`Math.max(W('a','${kinds[0]}'),W('b','${kinds[1]}'))`);
  addExpr(`W('a','${kinds[0]}')<W('b','${kinds[1]}')||W('a','${kinds[0]}')>=W('b','${kinds[1]}')`);
}

process.stderr.write(`${programs.length} programas\n`);

const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
    child.stdout.on("data", (chunk) => (out += chunk));
    child.stderr.on("data", (chunk) => (err += chunk));
    child.on("close", (status) => {
      clearTimeout(timer);
      resolve({ status, out, err });
    });
    child.stdin.end(source);
  });
}

(async () => {
  const sources = programs.map((body) => '"use strict";\n' + PRELUDE + body);
  const results = new Array(sources.length);
  let cursor = 0;
  const worker = async () => {
    while (cursor < sources.length) {
      const index = cursor++;
      results[index] = await runChild(sources[index]);
    }
  };
  await Promise.all(Array.from({ length: 10 }, worker));
  let kept = 0;
  let dropped = 0;
  const rows = [];
  for (let index = 0; index < sources.length; index++) {
    const { status, out: raw, err } = results[index];
    const out = status === 0 ? decodeResult(raw) : null;
    if (status !== 0 || out === null) {
      dropped++;
      process.stderr.write("erro de programa: " + JSON.stringify(programs[index]).slice(0, 200) + " " + err.slice(0, 120) + "\n");
      continue;
    }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(out)) {
      dropped++;
      process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(programs[index]).slice(0, 200) + "\n");
      continue;
    }
    kept++;
    rows.push({ source: sources[index], result: out });
  }
  process.stdout.write(emitFactored("operator_grid", rows));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
})();
