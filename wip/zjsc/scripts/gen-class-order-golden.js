// Gera tests/golden/class_order_bun.tsv: ordem de avaliação e semântica de classes, medida no bun 1.4.2.
// Só JS puro (sem APIs de host, sem import dinâmico). Famílias:
//   1. getters e setters privados (instância e estáticos) com herança, outra classe de mesmo corpo, Proxy, null, primitivo;
//   2. `super` em métodos de object literal (métodos, getters, setters, geradores, async, computed, arrow, eval) com
//      protótipo trocado, nulo ou Proxy, e erros de sintaxe fora de método;
//   3. `super` em static blocks, campos estáticos, métodos estáticos e de instância, com extends de classe e null;
//   4. `new.target` em funções chamadas por Reflect.construct com newTarget de várias formas (função, classe, bound,
//      Proxy, prototype nulo, arrow, método, gerador, builtin);
//   5. `class extends (class {})` e outras expressões de herança, com corpos variados;
//   6. a palavra `accessor` (campos, estático, privado, computed, ASI, nome `accessor`);
//   7. `#x in obj` em static, instância, bloco, campo e classe aninhada que sombreia o nome;
//   8. log da ordem de avaliação de computed keys, campos estáticos e blocos com efeitos.
// Programas já presentes nos goldens class_* são descartados. Cada programa roda num bun filho novo (6 em paralelo,
// timeout de 8 s). Colunas: fonte (JSON) e valor da global `R` (JSON).
// Uso: bun scripts/gen-class-order-golden.js > tests/golden/class_order_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}

const PRELUDE =
  'function S(v,d){d=d||0;var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":String(v);if(d>3)return "...";' +
  'if(Array.isArray(v))return "["+Array.from({length:v.length},(_,i)=>i in v?S(v[i],d+1):"<hole>").join(",")+"]";' +
  'return "{"+Reflect.ownKeys(v).map(k=>S(k)+":"+S(v[k],d+1)).join(",")+"}"}\n' +
  'function D(o,k){var d=Object.getOwnPropertyDescriptor(o,k);if(!d)return "none";return ("value" in d?"v="+S(d.value):"g="+S(d.get)+"/s="+S(d.set))+(d.writable===undefined?"":" w="+d.writable)+" e="+d.enumerable+" c="+d.configurable}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const J = JSON.stringify;
const exprs = [];
const add = (e) => exprs.push(e);
// Corpo `inner` roda com o log `l`; `ret` é avaliado depois (dentro do try) e vira S(...). Exceção entra como "!Nome: msg".
const RUN = (inner, ret) =>
  `T(()=>{var l=[];function L(x){l.push(String(x));return x}function K(x){l.push("k"+x);return x}class B{constructor(){L("B")}}var r;try{${inner};r=S(${ret || "0"})}catch(e){r="!"+e.name+": "+e.message}return l.join()+"|"+r})`;

// ---- 1. Getters e setters privados.
{
  const ops = [
    "return o.#a", "o.#a=5;return o.v", "o.#a+=2;return o.v", "o.#a++;return o.v", "return #a in o",
    "[o.#a]=[9];return o.v", "({x:o.#a}={x:7});return o.v", "o.#a??=3;return o.v", "o.#a||=3;return o.v",
  ];
  for (const place of ["inst", "static"]) {
    const st = place === "static" ? "static " : "";
    const targets = place === "static"
      ? ["C", "Sub", "Other", "{}", "null", "Object.create(C)", "new Proxy(C,{})", "1"]
      : ["new C", "new Sub", "{}", "null", "new Proxy(new C,{})", "Object.create(new C)", "new Other", "1"];
    const accessors = {
      get: `${st}get #a(){L("get");return this.v}`,
      set: `${st}set #a(x){L("set"+x);this.v=x}`,
      getset: `${st}get #a(){L("get");return this.v}${st}set #a(x){L("set"+x);this.v=x}`,
    };
    for (const kind of Object.keys(accessors)) for (const op of ops) for (const target of targets) {
      const body = `${st}v=1;${accessors[kind]}${st}op(o){${op}}`;
      const call = place === "static" ? `C.op(${target})` : `new C().op(${target})`;
      add(RUN(`class C{${body}}class Sub extends C{}class Other{${body}}`, call));
    }
  }
}

// ---- 2. super em object literals.
{
  const setup = "var P={x:'px',tag:'p',m(){return 'pm:'+this.tag},get g(){return 'pg:'+this.tag},set s(v){L('ps'+v+this.tag)}};" +
    "var Q={x:'qx',tag:'q',m(){return 'qm:'+this.tag},get g(){return 'qg'},set s(v){L('qs'+v)}};";
  const trap = "new Proxy(P,{get(t,k,r){L('get:'+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){L('set:'+String(k));return Reflect.set(t,k,v,r)}})";
  const protos = [
    ["__proto__:P,", ""], ["__proto__:P,", "Object.setPrototypeOf(o,Q);"], ["__proto__:P,", "Object.setPrototypeOf(o,null);"],
    ["", ""], [`__proto__:${trap},`, ""], ["__proto__:P,", "Object.setPrototypeOf(o,Object.create(Q));"],
  ];
  const kinds = [
    [(op) => `f(){return ${op}}`, "o.f()"],
    [(op) => `get f(){return ${op}}`, "o.f"],
    [(op) => `set f(v){L(${op})}`, "(o.f=1,'set')"],
    [(op) => `*f(){yield ${op}}`, "o.f().next().value"],
    [(op) => `async f(){L(${op})}`, "(o.f(),'called')"],
    [(op) => `[K('f')](){return ${op}}`, "o.f()"],
    [(op) => `f(){return (()=>${op})()}`, "o.f()"],
    [(op) => `f(){return eval(${J(op)})}`, "o.f()"],
  ];
  const ops = [
    "super.x", "super.m()", "super.g", "super['x']", "(super.s=3,this.tag)", "super.nope", "super.m.call({tag:'z'})",
    "(super.x=5,this.x+':'+Object.hasOwn(this,'x'))", "delete super.x", "typeof super.m",
  ];
  for (const [head, post] of protos) for (const [make, call] of kinds) for (const op of ops) {
    add(RUN(`${setup}var o={${head}tag:'o',${make(op)}};${post}`, call));
  }
  const snippets = [
    "({f:function(){return super.x}})", "({f:()=>super.x})", "function g(){return super.x}", "({f(){function h(){return super.x}}})",
    "({f(){return super()}})", "class Z{static{super()}}", "class Z{f(){super()}}", "class Z extends B{f(){super()}}",
    "class Z extends B{static{super.x}}", "({f(){return typeof new super.x}})", "({f(){return super.x`t`}})",
    "({get f(){return super.x}}).f", "({set f(v){super.x=v}}).f=1", "({f(){return super?.x}})", "({f(){return super[0]}})",
    "({f(){super.x=1;return super.x}}).f()", "(function(){return eval('super.x')})()", "({f(){return eval('super()')}}).f()",
  ];
  for (const src of snippets) add(RUN("", `(eval(${J(src)}),'ok')`));
}

// ---- 3. super em classes: static blocks, campos estáticos, métodos.
{
  const pre = "class A{static x='ax';static m(){return 'am:'+this.name};static get g(){return 'ag:'+this.name};static set s(v){L('as'+v+this.name)};pm(){return 'apm'}}";
  const heritages = [" extends A", " extends null", " extends class Q extends A{static x='qx'}", ""];
  const staticOps = [
    "super.x", "super.m()", "super.g", "(super.s=2,0)", "super.nope", "(super.y=3,Object.hasOwn(this,'y'))",
    "super.m.call({name:'z'})", "delete super.x", "typeof super.m", "super['g']",
  ];
  const instOps = ["super.pm()", "super.constructor.name", "super.nope", "(super.y=3,Object.hasOwn(this,'y'))"];
  const staticForms = [
    [(op) => `static{L(${op})}`, "0"],
    [(op) => `static f=L(${op})`, "0"],
    [(op) => `static f(){return ${op}}`, "C.f()"],
    [(op) => `static #p(){return ${op}}static c(){return C.#p()}`, "C.c()"],
    [(op) => `static get gg(){return ${op}}`, "C.gg"],
    [(op) => `static{L((()=>${op})())}`, "0"],
    [(op) => `static f=eval(${J(op)})`, "0"],
  ];
  const instForms = [
    [(op) => `f=L(${op})`, "new C"],
    [(op) => `f(){return ${op}}`, "new C().f()"],
    [(op) => `#p(){return ${op}}c(){return this.#p()}`, "new C().c()"],
  ];
  for (const her of heritages) {
    for (const [make, call] of staticForms) for (const op of staticOps) add(RUN(`${pre};class C${her}{${make(op)}}`, call));
    for (const [make, call] of instForms) for (const op of instOps) add(RUN(`${pre};class C${her}{${make(op)}}`, call));
  }
}

// ---- 4. new.target com Reflect.construct.
{
  const W = `L(new.target===undefined?"u":"t:"+(new.target.name||typeof new.target))`;
  const callees = [
    `function F(){${W}}`,
    `function F(){${W};return {r:1}}`,
    `function F(){${W};return 7}`,
    `function F(){${W};(()=>L("a:"+(new.target&&new.target.name)))()}`,
    `function F(){${W};eval("L('e:'+(new.target&&new.target.name))")}`,
    `class F{constructor(){${W}}}`,
    `class F extends B{constructor(){super();${W}}}`,
    `class F extends B{}`,
    `function G(){${W}}var F=G.bind(null)`,
    `function G(){${W}}var F=new Proxy(G,{construct(t,a,n){L("trap:"+(n===F?"proxy":n===G?"G":n.name));return Reflect.construct(t,a,n)}})`,
    `class F{x=L(new.target&&new.target.name)}`,
  ];
  const targets = [
    `function N(){}`, `class N{}`, `class N extends B{}`, `var N=function(){}.bind(null)`,
    `function M(){};var N=new Proxy(M,{get(t,k,r){L("pg:"+String(k));return Reflect.get(t,k,r)}})`,
    `function N(){};N.prototype=null`, `function N(){};N.prototype=5`,
    `function N(){};Object.defineProperty(N,"prototype",{value:{tag:1}})`, `var N=()=>{}`, `var N={m(){}}.m`,
    `var N=function*(){}`, `var N=async function(){}`, `var N=Math.max`, `var N=Object`, `var N=Array`, `var N=Map`,
    `var N=Symbol`, `var N=Function.prototype`, `function N(){};Object.setPrototypeOf(N,null)`,
    `var N=new Proxy(function(){},{})`, `var N=new Proxy(()=>{},{})`, `function N(){};N.prototype={__proto__:null,k:1}`,
  ];
  const obs = [
    "[typeof r,Object.getPrototypeOf(r)===N.prototype,Object.getPrototypeOf(r)===Object.prototype]",
    "[r instanceof N,Object.keys(r).join()]",
    "[l.length,r.constructor&&r.constructor.name]",
  ];
  for (const callee of callees) for (const target of targets) for (const ob of obs) {
    add(RUN(`${callee};${target};var r=Reflect.construct(F,[],N)`, ob));
  }
  const builtins = [["Array", "[3]"], ["Map", "[]"], ["Set", "[[1]]"], ["Error", "['m']"], ["Date", "[0]"], ["RegExp", "['a','g']"],
    ["Promise", "[function(){}]"], ["ArrayBuffer", "[4]"], ["Boolean", "[0]"], ["Object", "[]"], ["Number", "[1]"], ["String", "['s']"]];
  for (const [name, args] of builtins) for (const target of targets) {
    add(RUN(`${target};var r=Reflect.construct(${name},${args},N)`, "[Object.getPrototypeOf(r)===N.prototype,Object.prototype.toString.call(r)]"));
  }
  const forms = ["F()", "new F", "F.call({})", "Reflect.apply(F,{},[])", "Reflect.construct(F,[])", "Reflect.construct(F,[],F)",
    "Reflect.construct(F,[],Object)", "new (F.bind(null))", "new (new Proxy(F,{}))", "Reflect.construct(F,[],new Proxy(F,{}))", "new F.prototype.constructor"];
  for (const callee of callees) for (const form of forms) add(RUN(`${callee}`, `typeof (${form})`));
}

// ---- 5. extends com expressões de herança.
{
  const heritages = [
    "class{}", "class{static s=1}", "class A{constructor(){L('A')}}", "(L('h'),class{})", "(c=1)?class{}:class{}", "class extends B{}",
    "class{static name='x'}", "class{static #p=1;static t(o){return #p in o}}", "class{get x(){return 1}}", "(()=>class{})()",
    "class{constructor(){return {r:1}}}", "class extends null{}", "class{static [K('k')]=1}", "class{static{L('sb')}}", "function(){}",
    "async function(){}", "new Proxy(class{},{})", "class{}.prototype", "function*(){}", "Object",
  ];
  const bodies = [
    "{}", "{constructor(){super();L('c')}}", "{x=L('x')}", "{static y=L('y')}", "{static{L(this.name)}}", "{static a=this.s}",
    "{#p=1;has(o){return #p in o}}", "{get g(){return super.constructor.name}}", "{static m(){return super.t&&super.t(this)}}",
    "{constructor(){L(new.target.name)}}", "{static n2=this.name}", "{static z=super.s}",
  ];
  const ops = ["new C", "[C.name,Object.getPrototypeOf(C.prototype).constructor.name]", "[new C instanceof H,Object.getPrototypeOf(C)===H]", "Object.getOwnPropertyNames(C).join()"];
  for (const h of heritages) for (const b of bodies) for (const op of ops) {
    add(RUN(`var c,H;class C extends (H=${h}) ${b}`, op));
    add(RUN(`var c,H;var C=class extends (H=${h}) ${b}`, op));
  }
}

// ---- 6. accessor.
{
  const elems = [
    `accessor a=L("a")`, `static accessor a=L("a")`, `accessor #a=L("a");ra(){return this.#a}wa(v){this.#a=v}`,
    `accessor [K("k")]=L("v")`, `static accessor [K("k")]=L("v")`, `accessor a`, "accessor\na", `accessor=1`, `accessor(){return "m"}`,
    `static accessor`, `accessor accessor=L("aa")`, `get accessor(){return "g"}`, `accessor "str"=1`, `accessor 0=L("z")`,
    `static accessor #a=L("s");static ra(){return C.#a}`, `accessor a=L("a");accessor b=L("b")`,
  ];
  const ops = [
    `Reflect.ownKeys(C.prototype).map(String).join()`, `Reflect.ownKeys(new C).map(String).join()`, `D(C.prototype,"a")`,
    `(o=new C,o.a=5,[o.a,Reflect.ownKeys(o).length])`, `Object.getOwnPropertyDescriptor(C.prototype,"a").get.call({})`,
    `[(d=Object.getOwnPropertyDescriptor(C.prototype,"a")).get.name,d.set.name,d.get.length,d.set.length]`,
    `[new Sub().a,Reflect.ownKeys(new Sub).length]`, `JSON.stringify(new C)`, `(o=Object.freeze(new C),o.a=9,o.a)`,
    `(o=new Proxy(new C,{}),o.a)`, `(C.a=3,C.a)`,
  ];
  for (const her of ["", " extends B"]) for (const e of elems) for (const op of ops) {
    add(RUN(`var o,d;class C${her}{${e}}class Sub extends C{a=L("sub")}`, op));
  }
  for (const her of ["", " extends B"]) for (const e1 of elems) for (const e2 of elems) {
    if (e1.includes("#a") && e2.includes("#a")) continue;
    add(RUN(`class C${her}{${e1};${e2}};L("def");new C;L("end");new C`, "0"));
  }
}

// ---- 7. #x in obj.
{
  const decls = ["static #x=1", "static #x(){}", "static get #x(){return 1}", "static set #x(v){}", "#x=1", "#x(){}", "static accessor #x=1", "get #x(){return 1}set #x(v){}"];
  const sites = [
    ["static has(o){return #x in o}", "C.has"],
    ["has(o){return #x in o}", "new C().has"],
    ["static{H2=H2||(o=>#x in o)}", "H2"],
    ["static h=o=>#x in o", "C.h"],
    ["static has(o){return class{#x=1;static h(p){return #x in p}}.h(o)}", "C.has"],
    ["static has(o){return (class{#x=1;static h(p){return #x in p}}).h(new C)&&#x in o}", "C.has"],
  ];
  const targets = ["C", "Sub", "Object.create(C)", "new Proxy(C,{})", "{}", "Other", "new C", "C.prototype", "null", "1", "'s'", "function(){}",
    "Object.create(new C)", "Sub.prototype", "new Sub", "new Proxy(new C,{})"];
  for (const d of decls) for (const [site, fn] of sites) for (const t of targets) {
    add(RUN(`var H2;class C{${d};${site}}class Sub extends C{}class Other{${d};${site}}`, `${fn}(${t})`));
  }
}

// ---- 8. Ordem de avaliação de computed keys e campos estáticos.
{
  const pool = [
    ["a=L('a')", null], ["static s=L('s')", null], ["[K('1')]=L('v1')", null], ["static [K('2')]=L('v2')", null],
    ["static [{toString(){return K('ts')}}]=L('vt')", null], ["static get [K('g')](){return 1}", null], ["static{L('blk:'+typeof this.s)}", null],
    ["static z=L(this.s)", null], ["static u=L(typeof C)", null], ["#p=L('p')", "p"], ["static #q=L('q')", "q"],
    ["static async *[K('ag')](){}", null], ["*[K('gen')](){}", null], ["static x=(()=>this===C)()", null],
    ["static [Symbol.toPrimitive]=L('sp')", null], ["[(()=>{throw new Error('kt')})()]=1", null], ["static w=L(new.target)", null],
  ];
  const heritages = ["", " extends B", " extends (L('h'),B)", " extends B"];
  const ctors = ["", "", "", "constructor(){super();L('C')}"];
  const build = (idx, n) => {
    const privs = idx.map((x) => pool[x][1]).filter(Boolean);
    if (new Set(privs).size !== privs.length) return;
    const mode = n % 4;
    const body = idx.map((x) => pool[x][0]).join(";");
    const sub = n % 3 === 0 ? "class Sub extends C{y=L('y');static u2=L('u2');constructor(){L('S0');super();L('S1')}};L('def');new Sub;" : "L('def');new C;L('end');new C;";
    add(RUN(`class C${heritages[mode]}{${body};${ctors[mode]}};${sub}`, "0"));
  };
  let n = 0;
  for (let i = 0; i < 14; i++) for (let j = 0; j < 14; j++) for (let k = 0; k < 14; k++) {
    if ((i + 2 * j + 3 * k) % 2 !== 0) continue;
    build([i, j, k], n++);
  }
  for (let i = 0; i < pool.length; i++) for (let j = 0; j < pool.length; j++) { build([i, j], n++); build([i, j], n++); }
}

// ---- Execução.
const baseText = knownPrograms("class_order_bun.tsv", (name) => /^class_/.test(name)).join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
let dup = 0;
let dropped = 0;
const jobs = [];
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  jobs.push('"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`);
}
const DASH = new RegExp("[" + String.fromCharCode(0x2013) + String.fromCharCode(0x2014) + "]");
const PRELOAD = writeResultPreload();
function runChild(source) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 8000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", (d) => { err += d; });
    child.on("close", (code) => {
      clearTimeout(timer);
      const result = decodeResult(out);
      resolve(code === 0 && result !== null ? { out: result } : { err: err || "filho falhou" });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}
async function main() {
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: 6 }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await runChild(jobs[i]);
    }
  });
  await Promise.all(workers);
  const lines = [];
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.err !== undefined || r.out === "<undefined>") { dropped++; continue; }
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || DASH.test(r.out) || DASH.test(jobs[i]) || usesHostApi(jobs[i].slice(PRELUDE.length))) { dropped++; continue; }
    lines.push(JSON.stringify(jobs[i]) + "\t" + JSON.stringify(r.out));
  }
  process.stdout.write(emitFactoredLines("class_order", lines));
  process.stderr.write(`mantidos ${lines.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
