// Gera tests/golden/to_primitive_grid_bun.tsv: ToPrimitive observável em operadores binários e unários, medido no bun.
// Cobre + - * / % ** & | ^ << >> >>> < <= > >= == != instanceof in, ++/-- (prefixo e sufixo, em variável e em
// propriedade) e atribuição composta em propriedade (ordem get/valueOf/set), com operandos objeto que registram as
// chamadas de Symbol.toPrimitive (hint), valueOf e toString (ordem, quantidade, lançando, devolvendo objeto), Date
// (hint default vira string), Symbol e BigInt misturados e toPrimitive não chamável.
// Colunas: o sufixo do programa (JSON) e o valor da variável global `R` (JSON), no formato fatorado de golden-prelude.js.
// Cada programa roda num bun filho novo (no máximo 6 ao mesmo tempo, timeout de 8 s), sem APIs de host.
// Uso: bun scripts/gen-to-primitive-grid-golden.js > tests/golden/to_primitive_grid_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var S=v=>typeof v=="symbol"?String(v):typeof v=="bigint"?v+"n":Object.is(v,-0)?"-0":typeof v=="string"?JSON.stringify(v):String(v),' +
  'T=f=>{try{return S(f())}catch(e){return"!"+e.name+":"+e.message}};\n' +
  'var L=[],Q=(a,v,s)=>({valueOf(){L.push(a+"v");return v},toString(){L.push(a+"s");return s}}),' +
  'TP=(a,b)=>({[Symbol.toPrimitive](h){L.push(a+h);return b}}),U=f=>{var r=T(f);return L.join()+">"+r},' +
  'DT=a=>{var d=new Date(0);d.valueOf=function(){L.push(a+"v");return 11};d.toString=function(){L.push(a+"s");return"ds"};return d},' +
  'DP=a=>{var d=DT(a);d[Symbol.toPrimitive]=function(h){L.push(a+h);return Date.prototype[Symbol.toPrimitive].call(this,h)};return d};\n';

// Operandos objeto: `@` vira o rótulo ("a" para a esquerda, "b" para a direita).
const objectOperands = [];
for (const r of ["5", "'7'", "'x'", "7n", "true", "null", "undefined", "Symbol('s')", "{}", "[]", "-0", "NaN", "''", "2**53", "1.5"]) objectOperands.push(`TP(@,${r})`);
objectOperands.push(
  "Q(@,1,'2')", "Q(@,'3',{})", "Q(@,5n,'x')", "Q(@,{},'9')", "Q(@,{},{})", "Q(@,Symbol('q'),'q')", "Q(@,undefined,'u')", "Q(@,null,'n')", "Q(@,{},7n)", "Q(@,2n,3n)",
  "{valueOf(){L.push(@+'v');throw new RangeError('vo')},toString(){L.push(@+'s');return 1}}",
  "{valueOf(){L.push(@+'v');return {}},toString(){L.push(@+'s');throw new RangeError('ts')}}",
  "{valueOf:1,toString(){L.push(@+'s');return '8'}}",
  "{valueOf(){L.push(@+'v');return 4},toString:null}",
  "{valueOf:1,toString:2}",
  "Object.create(null)",
  "{[Symbol.toPrimitive]:undefined,valueOf(){L.push(@+'v');return 6}}",
  "{[Symbol.toPrimitive]:null,valueOf(){L.push(@+'v');return 6}}",
  "{[Symbol.toPrimitive]:1}", "{[Symbol.toPrimitive]:{}}", "{[Symbol.toPrimitive]:'x'}", "{[Symbol.toPrimitive]:true,valueOf(){return 1}}",
  "{get [Symbol.toPrimitive](){L.push(@+'g');throw new RangeError('g')}}",
  "{get [Symbol.toPrimitive](){L.push(@+'g');return h=>{L.push(@+h);return 3}}}",
  "{[Symbol.toPrimitive](h){L.push(@+h);throw new RangeError('tp')}}",
  "DT(@)", "DP(@)", "new Date(NaN)", "Object(1n)", "Object(Symbol('w'))", "Object(5)", "Object('s')", "Object(true)", "[1,2]", "[]", "({})", "function(){}",
  "new Proxy({},{get(t,k){L.push(@+'p:'+String(k));return undefined}})",
  "new Proxy({},{get(t,k){L.push(@+'p:'+String(k));return k==='valueOf'?()=>8:undefined}})",
  "new Proxy({},{get(t,k){L.push(@+'p:'+String(k));return k===Symbol.toPrimitive?h=>{L.push(@+h);return 9}:undefined}})",
);
const fillLabel = (operand, label) => "(" + operand.replace(/@/g, JSON.stringify(label)) + ")";

// Operandos da direita: primitivos e um objeto simples.
const rightOperands = ["2", "2n", "Symbol('r')"];
const leftPrimitives = ["2", "2n", "'3'"];

const binaryOps = ["+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>", ">>>", "<", "<=", ">", ">=", "==", "!="];
const compoundOps = ["+=", "-=", "*=", "/=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=", ">>>="];

const exprs = [];
const add = (...list) => exprs.push(...list);
const U = body => `U(()=>${body})`;

// ---- 1. Operando objeto na esquerda, primitivo ou objeto simples na direita.
for (const op of binaryOps) for (const a of objectOperands) for (const b of rightOperands) add(U(`${fillLabel(a, "a")} ${op} ${fillLabel(b, "b")}`));

// ---- 2. Primitivo na esquerda, operando objeto na direita.
for (const op of ["+", "-", "<", "==", "**", "&"]) for (const a of leftPrimitives) for (const b of objectOperands) add(U(`${a} ${op} ${fillLabel(b, "b")}`));

// ---- 3. Objeto dos dois lados (a ordem esquerda, direita, e quem lança primeiro).
const pairLeft = ["TP(@,5)", "TP(@,'7')", "TP(@,7n)", "TP(@,Symbol('s'))", "TP(@,{})", "Q(@,1,'2')", "Q(@,{},'9')", "DT(@)",
  "{valueOf(){L.push(@+'v');throw new RangeError('vo')}}", "Object.create(null)", "{[Symbol.toPrimitive]:1}", "Object(1n)"];
const pairRight = ["TP(@,3)", "TP(@,'x')", "TP(@,3n)", "Q(@,2,'3')", "Q(@,{},{})", "DP(@)", "{[Symbol.toPrimitive]:{}}", "{valueOf(){L.push(@+'v');throw new RangeError('vb')}}"];
for (const op of binaryOps) for (const a of pairLeft) for (const b of pairRight) add(U(`${fillLabel(a, "a")} ${op} ${fillLabel(b, "b")}`));

// ---- 4. Unários e ++/-- em variável e em propriedade.
const unaryForms = [
  v => `+${v}`, v => `-${v}`, v => `~${v}`, v => `!${v}`, v => `typeof ${v}`, v => `void ${v}`,
  v => `(()=>{var x=${v};var r=x++;return S(r)+'|'+S(x)})()`,
  v => `(()=>{var x=${v};var r=++x;return S(r)+'|'+S(x)})()`,
  v => `(()=>{var x=${v};var r=x--;return S(r)+'|'+S(x)})()`,
  v => `(()=>{var x=${v};var r=--x;return S(r)+'|'+S(x)})()`,
  v => `(()=>{var o={get p(){L.push('get');return ${v}},set p(n){L.push('set:'+typeof n+':'+S(n))}};var r=o.p++;return S(r)})()`,
  v => `(()=>{var o={get p(){L.push('get');return ${v}},set p(n){L.push('set:'+typeof n+':'+S(n))}};var r=++o.p;return S(r)})()`,
  v => `(()=>{var o={get p(){L.push('get');return ${v}},set p(n){L.push('set:'+typeof n+':'+S(n))}};var r=o.p--;return S(r)})()`,
  v => `(()=>{var o={get p(){L.push('get');return ${v}},set p(n){L.push('set:'+typeof n+':'+S(n))}};var r=--o.p;return S(r)})()`,
  v => `(()=>{var o={p:${v}};o.p++;return S(o.p)})()`,
  v => `(()=>{var o={p:${v}};--o.p;return S(o.p)})()`,
  v => `(()=>{var o=[${v}];o[0]++;return S(o[0])})()`,
];
for (const form of unaryForms) for (const a of objectOperands) add(U(form(fillLabel(a, "a"))));
for (const form of unaryForms) for (const a of ["5", "5n", "'5'", "Symbol('s')", "null", "undefined", "true", "'abc'", "-0", "2n**64n"]) add(U(form(a)));

// ---- 5. Atribuição composta: variável, propriedade com acessor (ordem get, valueOf, set) e elemento de array.
const compoundLeft = ["TP(@,5)", "TP(@,'7')", "TP(@,7n)", "TP(@,{})", "Q(@,1,'2')", "Q(@,{},'9')", "DT(@)", "DP(@)", "{valueOf(){L.push(@+'v');throw new RangeError('vo')}}", "Object.create(null)", "{[Symbol.toPrimitive]:1}", "5", "'5'", "5n", "Symbol('s')"];
const compoundRight = ["2", "'3'", "2n", "Symbol('r')", "TP(@,3)", "Q(@,2,'3')", "TP(@,3n)", "{valueOf(){L.push(@+'v');throw new RangeError('vb')}}"];
for (const op of ["+=", "-=", "**=", "&=", ">>>=", "%="]) for (const a of compoundLeft) for (const b of compoundRight) {
  const left = fillLabel(a, "a");
  const right = fillLabel(b, "b");
  add(U(`(()=>{var o={get p(){L.push('get');return ${left}},set p(n){L.push('set:'+typeof n+':'+S(n))}};o.p ${op} (L.push('rhs'),${right});return 'done'})()`));
  add(U(`(()=>{var x=${left};x ${op} (L.push('rhs'),${right});return S(x)})()`));
}
for (const op of compoundOps) for (const a of compoundLeft) {
  const left = fillLabel(a, "a");
  add(U(`(()=>{var o={p:${left}};o.p ${op} 2;return S(o.p)})()`), U(`(()=>{var o=[${left}];o[0] ${op} 2n;return S(o[0])})()`),
    U(`(()=>{var o={get p(){L.push('get');return 1},set p(n){L.push('set:'+typeof n+':'+S(n))}};o.p ${op} ${left};return 'done'})()`));
}

// ---- 6. instanceof e in.
const keyOperands = objectOperands.filter(o => !/Proxy/.test(o));
for (const a of keyOperands) {
  const k = fillLabel(a, "a");
  add(U(`${k} in {}`), U(`${k} in {a:1,5:2,x:3,'':4}`), U(`${k} in [1,2]`), U(`({}) instanceof ${k}`), U(`1 instanceof ${k}`), U(`${k} instanceof Object`), U(`${k} instanceof Function`),
    U(`'x' in ${k}`), U(`(()=>{var o={};o[${k}]=1;return Reflect.ownKeys(o).map(S).join()})()`), U(`({}) instanceof ({[Symbol.hasInstance]:${k}})`));
}
const hasInstance = [
  "{[Symbol.hasInstance](v){L.push('hi');return v}}", "{[Symbol.hasInstance](v){L.push('hi');return TP('r',1)}}", "{[Symbol.hasInstance]:1}", "{[Symbol.hasInstance]:null}", "{[Symbol.hasInstance]:undefined}",
  "{get [Symbol.hasInstance](){L.push('hg');throw new RangeError('hg')}}", "{[Symbol.hasInstance]:{}}", "{[Symbol.hasInstance](v){L.push('hi');throw new RangeError('hi')}}",
  "{prototype:{}}", "function(){}", "(()=>{})", "class{}", "Object.create(null)", "Symbol('s')", "1", "Function.prototype.bind.call(function(){})",
  "(()=>{var f=function(){};f.prototype=1;return f})()", "(()=>{var f=function(){};Object.defineProperty(f,'prototype',{get(){L.push('pg');return {}}});return f})()",
  "new Proxy(function(){},{get(t,k){L.push('p:'+String(k));return Reflect.get(t,k)}})",
];
const instanceLeft = ["{}", "1", "'s'", "Symbol('s')", "null", "TP('a',1)", "function(){}", "Object.create(null)", "[]"];
for (const r of hasInstance) for (const l of instanceLeft) add(U(`${l} instanceof (${r})`));
const inRight = ["{}", "1", "'s'", "null", "undefined", "Symbol('s')", "[]", "function(){}", "new Proxy({},{has(t,k){L.push('has:'+String(k));return true}})", "new Proxy({},{has(t,k){L.push('has:'+String(k));throw new RangeError('h')}})"];
const inLeft = ["'x'", "1", "Symbol('s')", "null", "TP('a','x')", "TP('a',1)", "TP('a',Symbol('k'))", "TP('a',{})", "Q('a',1,'k')", "Q('a','v',{})", "{valueOf(){L.push('av');return 1}}", "DT('a')", "DP('a')", "1n", "-0"];
for (const r of inRight) for (const l of inLeft) add(U(`${l} in ${r}`));

// ---- 7. Date: hint default vira string, number/string explícitos, e o método com receptores inválidos.
const dateHints = ["'default'", "'number'", "'string'", "'bad'", "undefined", "1", "{}", "Symbol('h')"];
const dateReceivers = ["DT('d')", "{valueOf(){L.push('dv');return 1},toString(){L.push('ds');return 's'}}", "{}", "1", "'s'", "null", "undefined", "Symbol('s')", "Object.create(null)", "{valueOf(){return {}},toString(){return {}}}", "{valueOf:1,toString:1}"];
for (const r of dateReceivers) for (const h of dateHints) add(U(`Date.prototype[Symbol.toPrimitive].call(${r},${h})`));
add(U("Date.prototype[Symbol.toPrimitive].name"), U("Date.prototype[Symbol.toPrimitive].length"), U("Date.prototype[Symbol.toPrimitive]()"),
  U("Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive).writable"), U("Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive).configurable"),
  U("Symbol.prototype[Symbol.toPrimitive].call(Object(Symbol('q')))===undefined"), U("typeof Symbol.prototype[Symbol.toPrimitive].call(Object(Symbol('q')))"),
  U("Symbol.prototype[Symbol.toPrimitive].call(1)"), U("Symbol.prototype[Symbol.toPrimitive].name"), U("Symbol.prototype[Symbol.toPrimitive].length"),
  U("Object.getOwnPropertyDescriptor(Symbol.prototype,Symbol.toPrimitive).writable"));
const dateOps = [
  "d+1", "1+d", "d-1", "d*2", "d/2", "d%7", "d**1", "d&1", "d|1", "d^1", "d<<1", "d>>1", "d>>>1", "d<1", "d>1", "d<=1", "d>=1", "d==1", "d==11", "d=='ds'", "d!=1", "d+''", "`${d}`", "d+1n", "d-1n", "d<1n", "d==11n", "+d", "-d", "~d", "d==d", "d<d",
];
for (const e of dateOps) for (const make of ["DT('a')", "DP('a')"]) add(U(`(d=>${e})(${make})`));
for (const e of ["d+1", "d-1", "d==1", "d<1", "`${d}`", "d+''", "+d", "String(d)", "Number(d)", "d.getTime()", "d.valueOf()===d.getTime()", "Object(d)==d"]) add(U(`(d=>${e})(new Date(7))`));

// ---- 8. Symbol e BigInt misturados.
const mixedValues = ["Symbol('s')", "Symbol()", "Object(Symbol('w'))", "1n", "-1n", "0n", "Object(2n)", "2n**64n", "TP('a',Symbol('t'))", "TP('a',3n)", "Q('a',4n,'s')", "Q('a',Symbol('q'),'s')"];
const mixedOther = ["1", "'1'", "null", "1n", "2n", "Symbol('o')", "{}", "TP('b',1)", "TP('b',1n)", "TP('b','s')"];
for (const op of [...binaryOps, "===", "!=="]) for (const a of mixedValues) for (const b of mixedOther) add(U(`${a} ${op} ${b}`));
for (const a of mixedValues) for (const f of [`Number(@)`, `String(@)`, `\`\${@}\``, `@+''`, `''+@`, `Math.abs(@)`, `Math.max(@,1)`, `isNaN(@)`, `parseInt(@)`, `BigInt(@)`, `[@].join()`, `({[@]:1})`, `JSON.stringify({a:@})`, `Object.is(@,@)`, `@?1:2`, `!@`, `typeof @`, `(x=>x==x)(@)`, `Boolean(@)`]) add(U(f.replace(/@/g, a)));

// ---- Execução.
const baseSet = new Set(knownPrograms("to_primitive_grid_bun.tsv", (name) => /bigint|coercion|symbol|operator|binary|unary|compound|primitive|date|equality|relational|arith|update|increment|instanceof/i.test(name) && name !== "to_primitive_grid_bun.tsv"));
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
const seen = new Set();
const jobs = [];
let dup = 0;
for (const expr of exprs) {
  if (usesHostApi(expr) || seen.has(expr)) continue;
  seen.add(expr);
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSet.has(source)) { dup++; continue; }
  jobs.push({ expr, source });
}

const run = job =>
  new Promise(resolve => {
    // Processo fresco por programa: o JSC reifica tabelas estáticas por ordem de acesso.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    const timer = setTimeout(() => { child.kill("SIGKILL"); }, 8000);
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => { clearTimeout(timer); const decoded = code === 0 ? decodeResult(out) : null; resolve(decoded !== null ? { ok: true, out: decoded } : { ok: false, err: err || "código " + code }); });
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
  process.stdout.write(emitFactoredLines("to_primitive_grid", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
