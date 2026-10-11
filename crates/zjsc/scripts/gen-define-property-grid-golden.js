// Gera tests/golden/define_property_grid_bun.tsv: ValidateAndApplyPropertyDescriptor em grade, medido no bun.
// Grade principal: estado inicial da propriedade (dado e acessor com todas as combinações de writable/enumerable/
// configurable e de get/set, ausente em objeto extensível e não extensível) x descritor novo (campos presentes ou
// ausentes, valor igual ou diferente, get/set iguais, diferentes ou undefined, mistura inválida get+value com o
// TypeError exato). Cada programa aplica o descritor com Object.defineProperty e com Reflect.defineProperty (true/false)
// em objetos novos e registra o descritor final com getOwnPropertyDescriptor. Alvos exóticos: length e índices de array,
// typed array, String boxed, arguments mapeado, funções (length, name, prototype).
// Cada programa roda num bun filho novo (stdin), sem APIs de host, no máximo 6 filhos ao mesmo tempo, timeout de 8 s.
// Uso: bun scripts/gen-define-property-grid-golden.js > tests/golden/define_property_grid_bun.tsv
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const { knownPrograms, emitFactored, sampleByHash } = require("./golden-prelude.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.stdout.write(String(globalThis.R));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn:"+v.name;' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n' +
  'var g1=function(){return 1},g2=function(){return 2},s1=function(v){},s2=function(v){};\n';

const TF = ["true", "false"];

// Descritor como texto: campos na ordem fixa; `undefined` como valor omite o campo.
function desc(fields) {
  const parts = [];
  for (const k of ["value", "writable", "get", "set", "enumerable", "configurable"]) {
    if (fields[k] !== undefined) parts.push(`${k}:${fields[k]}`);
  }
  return `{${parts.join(",")}}`;
}

// Todas as combinações de um mapa campo -> lista de textos (a lista pode conter undefined = campo ausente).
function product(spec) {
  let acc = [{}];
  for (const key of Object.keys(spec)) {
    const next = [];
    for (const base of acc) for (const v of spec[key]) next.push({ ...base, [key]: v });
    acc = next;
  }
  return acc.map(desc);
}

const wec = { writable: [undefined, ...TF], enumerable: [undefined, ...TF], configurable: [undefined, ...TF] };
const wecSmall = { writable: [undefined, ...TF], enumerable: [undefined, "false"], configurable: [undefined, "false"] };

// ---- grade principal ----
const gridDescs = [
  ...product({ value: [undefined, "1", "2"], ...wec }),
  ...product({ get: [undefined, "g1", "g2", "undefined"], set: [undefined, "s1", "s2", "undefined"], enumerable: [undefined, ...TF], configurable: [undefined, ...TF] }),
  ...product({ get: ["g1"], value: ["1"], configurable: [undefined, "false", "true"] }),
  ...product({ set: ["s1"], value: ["1"], configurable: [undefined, "false", "true"] }),
  ...product({ get: ["g1"], writable: ["true", "false"], configurable: [undefined, "false"] }),
  ...product({ set: ["s1"], writable: ["true", "false"], configurable: [undefined, "false"] }),
  ...product({ get: ["undefined"], value: ["undefined"], enumerable: [undefined, "true"] }),
  "{get:1}", "{set:'x'}", "{get:null}", "{set:{}}", "{get:g1,set:3}", "{get:g1,set:s1,value:2,writable:true}",
];

function mkDataState(w, e, c, value = "1") {
  return `(function(){var o={};Object.defineProperty(o,'k',{value:${value},writable:${w},enumerable:${e},configurable:${c}});return o})()`;
}
function mkAccessorState(get, set, e, c) {
  const fields = [];
  if (get) fields.push("get:g1");
  if (set) fields.push("set:s1");
  return `(function(){var o={};Object.defineProperty(o,'k',{${fields.join(",")},enumerable:${e},configurable:${c}});return o})()`;
}

const states = [];
for (const w of TF) for (const e of TF) for (const c of TF) states.push(mkDataState(w, e, c));
for (const [g, s] of [[1, 0], [0, 1], [1, 1]]) for (const e of TF) for (const c of TF) states.push(mkAccessorState(g, s, e, c));
states.push("{}", "Object.preventExtensions({})");
states.push("(function(){var o={};Object.defineProperty(o,'k',{value:1});return Object.preventExtensions(o)})()");
states.push("(function(){var o={};Object.defineProperty(o,'k',{value:1,writable:true,configurable:true});return Object.preventExtensions(o)})()");

// Observação de um alvo: mesmo descritor por Object.defineProperty e por Reflect.defineProperty, em dois objetos novos.
function observe(make, key, descText, view) {
  return `T(()=>{var o=${make},p=${make};` +
    `var a=T(()=>{Object.defineProperty(o,${key},${descText});return "ok"});var va=${view("o", key)};` +
    `var b=T(()=>Reflect.defineProperty(p,${key},${descText}));var vb=${view("p", key)};` +
    `return a+" | "+va+" | "+b+" | "+vb})`;
}

const exprs = [];
const plainView = (o, k) => `D(${o},${k})`;
// Amostragem determinística: o tempo de geração (um bun por programa) manda no tamanho da grade. Fica 1 em cada `step`
// descritores, escolhidos por hash (sampleByHash) do `salt` (o contexto) mais o descritor: contextos diferentes veem
// subconjuntos diferentes, e a escolha não depende da posição do descritor na lista.
const thin = (list, step, salt) => sampleByHash(list, Math.ceil(list.length / step), (d) => salt + "\0" + d);
states.forEach((state) => { for (const d of thin(gridDescs, 3, state)) exprs.push(observe(state, "'k'", d, plainView)); });

// Valores especiais (SameValue): 0, -0 e NaN.
{
  const specialDescs = product({ value: ["0", "-0", "NaN", "1"], writable: [undefined, ...TF] });
  for (const value of ["0", "-0", "NaN"]) for (const w of TF) for (const c of TF) {
    for (const d of thin(specialDescs, 2, value + w + c)) exprs.push(observe(mkDataState(w, "true", c, value), "'k'", d, plainView));
  }
}

// ---- exóticos ----
function dataDescs(values) {
  return [...product({ value: values, ...wecSmall }),
    ...product({ get: [undefined, "g1", "undefined"], set: [undefined, "s1"], configurable: [undefined, "false", "true"] }),
    "{get:g1,value:1}", "{set:s1,writable:true}", "{}"];
}
function exotic(make, keys, values, view) {
  keys.forEach((key) => { for (const d of thin(dataDescs(values), 4, make + "\0" + key)) exprs.push(observe(make, key, d, view)); });
}
const stateView = (o, k) => `S(${o})+" "+D(${o},${k})+" len="+${o}.length`;

// array: length e índices
const arrayShapes = [
  "[1,2,3]", "[]", "[1,,3]",
  "(function(){var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});return a})()",
  "Object.freeze([1,2,3])",
  "(function(){var a=[1,2,3];Object.defineProperty(a,'1',{configurable:false});return a})()",
  "Object.preventExtensions([1,2,3])",
];
for (const shape of arrayShapes) {
  exotic(shape, ["'length'"], ["0", "1", "2", "3", "5", "'2'", "-1", "1.5", "NaN", "undefined", "4294967295", "4294967296", "{valueOf(){return 1}}"], stateView);
  exotic(shape, ["'0'", "'1'", "'3'", "'5'", "'4294967294'", "'01'", "'-0'"], ["1", "9"], stateView);
}

// typed array
for (const make of ["new Uint8Array(3)", "new Float64Array(2)", "new Int8Array(0)", "Object.freeze(new Uint8Array(0))"]) {
  exotic(make, ["'0'", "'2'", "'3'", "'-0'", "'1.5'", "'length'", "'foo'", "'-1'", "'Infinity'"], ["1", "300", "-1", "undefined", "'7'"],
    (o, k) => `S(Array.from(${o}))+" "+D(${o},${k})`);
}

// String boxed
for (const make of ["new String('abc')", "new String('')", "Object.preventExtensions(new String('ab'))"]) {
  exotic(make, ["'0'", "'1'", "'2'", "'3'", "'length'", "'x'"], ["'a'", "'b'", "1", "undefined"], (o, k) => `D(${o},${k})+" "+Reflect.ownKeys(${o}).join()`);
}

// arguments mapeado: o parâmetro a acompanha o índice 0 até o mapeamento ser desfeito
{
  const keys = ["'0'", "'1'", "'2'", "'length'", "'callee'"];
  for (const key of keys) for (const d of dataDescs(["1", "9"])) {
    const body = (call) => `(function(a,b){var r=T(()=>{${call};return "ok"});var x=D(arguments,${key})+S([a,b]);a=7;x+=D(arguments,${key})+S([a,b]);arguments[0]=8;x+=S([a,b])+D(arguments,'0');return r+"|"+x})(1,2)`;
    exprs.push(`${body(`Object.defineProperty(arguments,${key},${d})`)}+" || "+${body(`if(!Reflect.defineProperty(arguments,${key},${d}))throw "false"`)}`);
  }
}

// funções: length, name, prototype
for (const make of [
  "function f(a,b){}", "(a,b,c)=>0", "class C{}", "class E{static name(){}}", "function(x){}.bind(null)", "Math.max", "Array",
  "async function(){}", "function*(){}", "({m(){}}).m",
]) {
  exotic(`(${make})`, ["'length'", "'name'", "'prototype'"], ["0", "5", "'x'", "undefined"],
    (o, k) => `D(${o},${k})+" "+Reflect.ownKeys(${o}).join()`);
}

// ---- dedup e execução ----
const known = new Set();
for (const program of knownPrograms("define_property_grid_bun.tsv", (file) => file !== "define_property_grid_bun.tsv")) known.add(program);

const seen = new Set();
const jobs = [];
let repeated = 0;
for (const expr of exprs) {
  const source = PRELUDE + `globalThis.R = ${expr}`;
  if (seen.has(source)) continue;
  seen.add(source);
  if (known.has(source)) { repeated++; continue; }
  jobs.push({ expr, source });
}

function runChild(job) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, 8000);
    child.stdout.on("data", (chunk) => { out += chunk; });
    child.on("close", (code) => { clearTimeout(timer); resolve({ timedOut, code, out }); });
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
  await Promise.all(Array.from({ length: 6 }, worker));
  const dash = String.fromCharCode(0x2014);
  const endash = String.fromCharCode(0x2013);
  let timeouts = 0;
  let dropped = 0;
  const rows = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.timedOut) { timeouts++; continue; }
    if (r.code !== 0 || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || r.out.includes(dash) || r.out.includes(endash)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(jobs[i].expr).slice(0, 160) + "\n");
      continue;
    }
    rows.push({ source: jobs[i].source, result: r.out });
  }
  process.stdout.write(emitFactored("define_property_grid", rows));
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, timeouts de 8 s ${timeouts}, repetidos dos goldens existentes ${repeated}\n`);
}
main();
