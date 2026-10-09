// Gera tests/golden/arguments_grid_bun.tsv: o objeto `arguments` em grade, medido no bun 1.4.2.
// Grade: listas de parâmetros (simples, default, rest, destructuring, duplicados em sloppy) x modo (sloppy, strict por
// diretiva externa, 'use strict' interno) x corpo (leitura/escrita e reflexo no parâmetro, length, delete,
// defineProperty em índice mapeado, keys, spread, callee, Symbol.iterator, toStringTag, e capturas por arrow, eval
// direto, função interna e closure em laço) x chamadas com menos/mais argumentos. Programas iguais a uma fonte já
// presente em tests/golden/*.tsv são descartados; programa que estoura 5 s no bun é descartado e contado.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Uso: bun scripts/gen-arguments-grid-golden.js > tests/golden/arguments_grid_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

// [texto, nomes visíveis, simples, duplicado]
const params = [
  ["", [], true, false], ["a", ["a"], true, false], ["a,b", ["a", "b"], true, false], ["a,b,c", ["a", "b", "c"], true, false],
  ["a,b,c,d", ["a", "b", "c", "d"], true, false], ["a=1", ["a"], false, false], ["a,b=2", ["a", "b"], false, false],
  ["a,b=a", ["a", "b"], false, false], ["...r", ["r"], false, false], ["a,...r", ["a", "r"], false, false],
  ["{x}", ["x"], false, false], ["[x]", ["x"], false, false], ["{x},y", ["x", "y"], false, false], ["a,a", ["a"], true, true],
  ["a,a,b", ["a", "b"], true, true], ["a=arguments.length", ["a"], false, false], ["a,b=arguments[0]", ["a", "b"], false, false],
  ["a=arguments[1],b", ["a", "b"], false, false],
];

const SL = "S([].slice.call(arguments))";
const defs = (n) => `Object.defineProperty(arguments,'${n}',`;
// [corpo, apenasSloppy]; @0 e @1 são os dois primeiros nomes, @O a observação dos parâmetros, @L a cópia de arguments.
const bodies = [
  ["return @L+@O"], ["arguments[0]=9;return @L+@O"], ["arguments[1]=8;return @L+@O"], ["arguments[5]=7;return arguments.length+@L+@O"],
  ["@0=5;return @L+@O"], ["@1=6;return @L+@O"], ["@0=5;return S(arguments[0])+@O"], ["@1=6;return S(arguments[1])+@O"],
  ["arguments[0]++;return @O+@L"], ["arguments[0]=undefined;return @L+@O"], ["arguments.length=1;return @L+arguments.length+@O"],
  ["arguments.length=5;return arguments.length+@L"], ["arguments.length=-1;return arguments.length+@L"],
  ["delete arguments[0];arguments[0]=3;return @L+@O"], ["delete arguments[0];@0=4;return @L+@O"], ["delete arguments[1];@1=4;return @L+@O"],
  ["return S(delete arguments[0])+S(delete arguments[9])+@L"], ["return S(delete arguments.length)+S(arguments.length)"],
  [`${defs(0)}{value:7});return @L+@O`], [`${defs(0)}{writable:false});@0=4;return @L+@O`],
  [`${defs(0)}{writable:false,value:7});@0=4;return @L+@O`], [`${defs(0)}{writable:false});arguments[0]=4;return @L+@O`],
  [`${defs(0)}{get(){return 5}});@0=4;return @L+@O`], [`${defs(0)}{get(){return 5}});arguments[0]=3;return @L+@O`],
  [`${defs(0)}{enumerable:false});return S(Object.keys(arguments))+@L`], [`${defs(0)}{configurable:false});return S(delete arguments[0])+@L+@O`],
  [`${defs(1)}{value:7});@1=1;return @L+@O`], [`${defs(1)}{writable:false});@1=1;return @L+@O`],
  [`${defs(0)}{value:3,writable:false});${defs(0)}{writable:true});@0=8;return @L+@O`],
  [`${defs(2)}{value:3});return @L+@O`], [`${defs('length')}{value:1});return @L+arguments.length`],
  [`${defs('length')}{enumerable:true});return S(Object.keys(arguments))`],
  ["Object.freeze(arguments);@0=4;return @L+@O"], ["Object.seal(arguments);@0=4;arguments[0]=2;return @L+@O"],
  ["Object.preventExtensions(arguments);arguments[7]=1;@0=4;return @L+@O"],
  ["return S(Object.keys(arguments))"], ["return S(Object.getOwnPropertyNames(arguments))"], ["return S(Reflect.ownKeys(arguments).map(String))"],
  ["return S(Object.entries(arguments))"], ["var k=[];for(var i in arguments)k.push(i);return S(k)"], ["return S([...arguments])"],
  ["return S(Array.from(arguments))"], ["return S(Array.prototype.map.call(arguments,x=>x*2))"], ["return JSON.stringify(arguments)"],
  ["return typeof arguments[Symbol.iterator]+S(arguments[Symbol.iterator]===Array.prototype.values)"],
  ["return D(arguments,Symbol.iterator)"], ["return S(Object.prototype.toString.call(arguments))+S(arguments[Symbol.toStringTag])"],
  ["return S(Object.getPrototypeOf(arguments)===Object.prototype)+S(Array.isArray(arguments))+typeof arguments"],
  ["return D(arguments,'callee')"], ["return D(arguments,'length')"], ["return D(arguments,'0')"], ["return D(arguments,'1')"],
  ["return S(arguments.callee===f)"], ["return typeof arguments.callee"], ["arguments.callee=1;return typeof arguments.callee"],
  ["return S(delete arguments.callee)+S('callee' in arguments)"], ["return S(Object.getOwnPropertyDescriptor(arguments,'callee').get===Object.getOwnPropertyDescriptor(arguments,'callee').set)"],
  ["return S([typeof f.arguments,f.arguments===arguments])"], ["return S(Object.hasOwn(arguments,'callee'))+S(Object.hasOwn(arguments,Symbol.iterator))"],
  ["var g=()=>arguments[0];return S(g())+@O"], ["var g=()=>arguments.length;return S(g())+@O"], ["var g=()=>()=>arguments[1];return S(g()())"],
  ["var g=()=>{arguments[0]=9};g();return @L+@O"], ["var g=()=>{@0=9};g();return @L+@O"], ["var g=()=>()=>{arguments[1]=9};g()();return @L+@O"],
  ["var g=(x)=>arguments[x];return S(g(0))+S(g(1))"],
  ["return S(eval('arguments.length'))"], ["return S(eval('arguments[0]'))"], ["eval('arguments[0]=7');return @L+@O"], ["eval('@0=7');return @L+@O"],
  ["return S(eval('(()=>arguments[0])()'))"], ["return S(eval('[].slice.call(arguments)'))"], ["return S(eval('var arguments=5;arguments'))+@L"],
  ["return S(eval('typeof arguments'))+S(eval('arguments')===arguments)"],
  ["function inner(){return arguments.length}return S(inner(1,2,3))+S(arguments.length)"],
  ["function inner(){arguments[0]=9;return arguments[0]}return S(inner(1))+@L"],
  ["var inner=function(){return S([].slice.call(arguments))};return inner(...arguments)+@L"],
  ["return S((function(){return arguments.length})(1))+S(arguments.length)"], ["return S(((x)=>arguments.length)(1,2,3,4))"],
  ["var o={m(){return arguments.length}};return S(o.m(1,2))+S(arguments.length)"],
  ["var fs=[];for(var i=0;i<3;i++)fs.push(()=>arguments[i]);return S(fs.map(g=>g()))"],
  ["var fs=[];for(let i=0;i<3;i++)fs.push(()=>arguments[i]);return S(fs.map(g=>g()))"],
  ["var fs=[];for(let i=0;i<arguments.length;i++)fs.push(()=>arguments[i]);arguments[0]=9;return S(fs.map(g=>g()))"],
  ["var fs=[];for(var i=0;i<2;i++)fs.push(function(){return arguments.length+i});return S(fs.map(g=>g(1)))"],
  ["return S(Math.max.apply(null,arguments))+S(Math.max(...arguments))"],
  ["return S(f.length)+S(arguments.length)"],
  ["arguments=1;return typeof arguments+@O", true], ["var arguments=1;return typeof arguments+@O", true], ["var arguments;return @L+@O", true],
  ["function arguments(){}return typeof arguments+@O", true], ["var arguments=7;@0=4;return S(arguments)+@O", true],
  ["return S(eval('var arguments=5;arguments'))+@L", true],
];

const calls = ["", "1", "1,2", "1,2,3", "1,2,3,4,5", "undefined", "{x:1},[2]", "1,undefined,3", "...[1,2]"];

const exprs = [];
for (let pi = 0; pi < params.length; pi++) {
  const [text, names, simple, dup] = params[pi];
  for (let bi = 0; bi < bodies.length; bi++) {
    const [raw, sloppyOnly] = bodies[bi];
    const hasNames = names.length > 0;
    const n0 = hasNames ? names[0] : "g0";
    const n1 = names.length > 1 ? names[1] : n0;
    const body = raw.replace(/@([01OL])/g, (_, k) =>
      k === "0" ? n0 : k === "1" ? n1 : k === "O" ? (hasNames ? `S([${names.join()}])` : "''") : SL);
    const decl = hasNames ? "" : "var g0;";
    for (let mi = 0; mi < 3; mi++) {
      if (mi > 0 && sloppyOnly) continue;
      if (mi > 0 && dup) continue;
      if (mi === 2 && !simple) continue;
      const h = pi * 7 + bi * 3 + mi * 5;
      for (const ci of new Set([h % calls.length, (h + 4) % calls.length])) {
        const args = calls[ci];
        let fn;
        if (mi === 0) fn = `function f(${text}){${decl}${body}}return f(${args})`;
        else if (mi === 1) fn = `"use strict";function f(${text}){${decl}${body}}return f(${args})`;
        else fn = `function f(${text}){"use strict";${decl}${body}}return f(${args})`;
        exprs.push(`T(()=>{${fn}})`);
      }
    }
  }
}
// 'use strict' interno com parâmetros não simples: o erro de sintaxe aparece em tempo de execução, por eval.
for (const [text] of params.filter((p) => !p[2])) {
  exprs.push(`T(()=>eval(${JSON.stringify(`(function f(${text}){"use strict";return arguments.length})(1)`)}))`);
  exprs.push(`T(()=>new Function(${JSON.stringify(text)},${JSON.stringify('"use strict";return arguments.length')})(1,2))`);
}
for (const text of ["a,a", "a,b,a"]) {
  exprs.push(`T(()=>eval(${JSON.stringify(`"use strict";(function f(${text}){return arguments.length})(1)`)}))`);
  exprs.push(`T(()=>eval(${JSON.stringify(`(function f(${text}){"use strict";return arguments.length})(1)`)}))`);
  exprs.push(`T(()=>eval(${JSON.stringify(`((${text})=>arguments.length)(1)`)}))`);
  exprs.push(`T(()=>new Function(${JSON.stringify(text)},"return S(arguments.length)")(1,2,3))`);
}
for (const p of ["arguments", "eval"]) {
  exprs.push(`T(()=>eval(${JSON.stringify(`"use strict";(function f(${p}){return 1})(1)`)}))`);
  exprs.push(`T(()=>eval(${JSON.stringify(`(function f(${p}){return typeof ${p}})(1)`)}))`);
}

// Fontes já presentes nos goldens existentes.
const known = new Set();
const goldenDir = path.join(__dirname, "..", "tests", "golden");
for (const program of knownPrograms("arguments_grid_bun.tsv", (file) => !(!file.endsWith(".tsv") || file === "arguments_grid_bun.tsv"))) known.add(JSON.stringify(program));

const seen = new Set();
const jobs = [];
let dup = 0;
for (const expr of exprs) {
  const source = PRELUDE + `globalThis.R = ${expr}`;
  const key = JSON.stringify(source);
  if (seen.has(key)) continue;
  seen.add(key);
  if (known.has(key)) { dup++; continue; }
  jobs.push({ expr, source, key });
}

let timeouts = 0;
let dropped = 0;

const PRELOAD = writeResultPreload();
function runChild(job) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 5000);
    child.stdout.on("data", (chunk) => { out += chunk; });
    child.on("close", (code) => {
      clearTimeout(timer);
      const decoded = decodeResult(out);
      resolve({ timedOut, code: code === 0 && decoded === null ? 1 : code, out: decoded === null ? "" : decoded });
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
  let kept = 0;
  const rows = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.timedOut) { timeouts++; continue; }
    if (r.code !== 0 || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun|—|–/i.test(r.out)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(jobs[i].expr).slice(0, 160) + "\n");
      continue;
    }
    kept++;
    rows.push({ source: jobs[i].source, result: r.out });
  }
  process.stdout.write(emitFactored("arguments_grid", rows));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, timeouts de 5 s ${timeouts}, repetidos dos goldens existentes ${dup}\n`);
}
main();
