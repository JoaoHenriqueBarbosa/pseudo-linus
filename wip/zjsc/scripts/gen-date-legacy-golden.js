// Gera tests/golden/date_legacy_bun.tsv: Date legado e conversão, medido no bun 1.4.2 com TZ=UTC fixo no filho.
// Cobre setters e getters locais e UTC em grade (overflow de mês, dia 0/-1/32, setHours com 4 argumentos, setFullYear
// em Date inválida, setTime NaN), new Date com 0 a 7 argumentos e tipos (string, número, Date, objeto com
// valueOf/toString/@@toPrimitive, BigInt e Symbol lançando TypeError), Date.prototype[@@toPrimitive] com hints
// inválidos, toJSON em objetos genéricos e com toISOString próprio, subclasses, aritmética e comparação de Dates,
// RangeError "Invalid time value", anos extremos de ±271821 e anos 0 a 99 no construtor contra Date.UTC.
// Complementa date_utc e date_core com combinações que eles não têm (programas repetidos nesses goldens são descartados).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-object-edge-golden.js.
// Cada programa roda num bun filho novo, sem APIs de host, com TZ=UTC.
// Uso: bun scripts/gen-date-legacy-golden.js > tests/golden/date_legacy_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const path = require("path");
const { spawnSync } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'function S(v){var t=typeof v;if(t==="string")return JSON.stringify(v);if(t==="symbol")return String(v);if(t==="function")return "fn";' +
  'if(v instanceof Date)return isNaN(v)?"Date(NaN)":"Date("+v.toISOString()+")";' +
  'if(v===null||t!=="object")return Object.is(v,-0)?"-0":t==="bigint"?v+"n":String(v);' +
  'if(Array.isArray(v))return "["+v.map(S).join(",")+"]";return "{"+Object.keys(v).map(k=>k+":"+S(v[k])).join(",")+"}"}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const E = body => add(`T(()=>${body})`);

// ---- 1. Setters locais e UTC em grade: retorno do setter e instante resultante.
const bases = {
  a: "new Date(2024,0,31,10,20,30,400)",
  b: "new Date(2023,11,31,23,59,59,999)",
  c: "new Date(2024,1,29,0,0,0,0)",
  d: "new Date(NaN)",
  e: "new Date(0)",
  f: "new Date(1969,11,31,23,59,59,999)",
};
const setters = {
  setMonth: [[0], [1], [11], [12], [13], [-1], [-13], [25], [1, 0], [1, 31], [1, 32], [1, -1], [1.9], [-0.5], [NaN], [undefined], ["2"], [null], [], [2 ** 31], [1e10], [0, 0, 5]],
  setDate: [[0], [-1], [-30], [1], [28], [29], [30], [31], [32], [60], [365], [-365], [1.9], [-0.9], [NaN], [undefined], ["15"], [null], [], [Infinity], [1e12]],
  setHours: [[0], [23], [24], [25], [-1], [48], [10, 61], [10, 0, 61], [10, 0, 0, 1000], [1, 2, 3, 4], [1, 2, 3, 4, 5], [1, NaN], [1, 2, NaN], [1, 2, 3, NaN], [NaN], [], [1.5, 2.5, 3.5, 4.5], ["5", "6", "7", "8"], [-25, -61, -61, -1001], [undefined, 1]],
  setMinutes: [[0], [59], [60], [61], [-1], [-61], [1, 61], [1, 2, 1001], [1, 2, 3, 4], [NaN], [], [1.5], [1, undefined], [1e6]],
  setSeconds: [[0], [59], [60], [61], [-1], [-61], [1, 1001], [1, 2, 3], [NaN], [], [1.5], [86400], [1, null]],
  setMilliseconds: [[0], [999], [1000], [1001], [-1], [-1001], [86400000], [NaN], [], [1.5], [-0.5], ["7"], [1e15], [Infinity]],
  setFullYear: [[2024], [0], [99], [100], [-1], [1970], [275760], [275761], [-271821], [-271822], [2024, 1, 29], [2023, 1, 29], [2024, 12, 1], [2024, -1, 1], [2024, 0, 0], [2024, 0, 32], [NaN], [], [2024.9], [2024, NaN], [2024, 1, NaN], [-0], ["2025"]],
};
const mk = (name, args) => `${name}(${args.map(a => (a === undefined ? "undefined" : typeof a === "string" ? JSON.stringify(a) : Object.is(a, -0) ? "-0" : String(a))).join(",")})`;
for (const [bk, base] of Object.entries(bases)) {
  for (const [name, list] of Object.entries(setters)) {
    for (const args of list) {
      for (const prefix of ["", "UTC"]) {
        const m = prefix ? name.replace("set", "setUTC") : name;
        add(`T(()=>{var d=${base};var r=d.${mk(m, args)};return S(r)+" "+S(d)})`);
      }
    }
  }
}

// ---- 2. setTime, setYear/getYear e retornos.
for (const base of Object.values(bases)) {
  for (const v of ["NaN", "0", "-0", "8.64e15", "8.64e15+1", "-8.64e15", "-8.64e15-1", "1.9", "-1.9", "'12'", "undefined", "null", "Infinity", "{valueOf(){return 5}}", "new Date(7)", "true", "[]", "''", "1e20"]) {
    add(`T(()=>{var d=${base};var r=d.setTime(${v});return S(r)+" "+S(d)+" "+d.getTime()})`);
  }
  for (const v of ["0", "1", "99", "100", "2024", "-1", "NaN", "1900", "1999.5", "'70'"]) {
    add(`T(()=>{var d=${base};var r=d.setYear(${v});return S(r)+" "+S(d)+" "+d.getYear()})`);
  }
}
add("T(()=>new Date(0).setTime())", "T(()=>Date.prototype.setTime.call({},1))", "T(()=>Date.prototype.setHours.call(1,1))", "T(()=>Date.prototype.setMonth.call(null,1))", "T(()=>Date.prototype.getTime.call(Date.prototype))", "T(()=>Date.prototype.valueOf.call(new Proxy(new Date(0),{})))");

// ---- 3. Getters em grade de instantes.
const stamps = [
  "0", "-1", "1", "86399999", "86400000", "-86400001", "951782400000", "951868800000", "4107542400000", "-62135596800000",
  "-62167219200000", "-62167219200001", "-62198755200000", "253402300799999", "253402300800000", "8.64e15", "-8.64e15", "8.64e15+1",
  "1e12", "1.7e12", "-1e12", "1234567890123", "NaN", "1.5", "-1.5", "0.999", "2**31*1000", "-(2**31)*1000", "951782400000.9", "-0.5",
];
const getters = ["getFullYear", "getUTCFullYear", "getMonth", "getUTCMonth", "getDate", "getUTCDate", "getDay", "getUTCDay", "getHours", "getUTCHours", "getMinutes", "getUTCMinutes", "getSeconds", "getUTCSeconds", "getMilliseconds", "getUTCMilliseconds", "getTime", "valueOf", "getTimezoneOffset", "getYear"];
for (const s of stamps) for (const g of getters) add(`T(()=>new Date(${s}).${g}())`);
for (const g of getters) {
  add(`T(()=>Date.prototype.${g}.call({}))`, `T(()=>Date.prototype.${g}.call(undefined))`, `T(()=>Date.prototype.${g}.length+${JSON.stringify(g)})`, `T(()=>Date.prototype.${g}.name)`);
  add(`T(()=>Date.prototype.${g}.call(Date.prototype))`, `T(()=>Date.prototype.${g}.call(Object.create(Date.prototype)))`);
}

// ---- 4. new Date com 0 a 7 argumentos.
const nums = ["0", "1", "-1", "11", "12", "13", "99", "100", "1970", "2024", "-1", "275760", "NaN", "1.9", "-1.9", "'5'", "undefined", "null", "true", "'x'", "Infinity", "1e10"];
for (let n = 2; n <= 7; n++) {
  const vals = ["0", "1", "11", "12", "-1", "99", "100", "NaN", "1.9", "'5'", "undefined", "null", "31", "32", "60", "1000"];
  for (const y of ["1970", "2024", "99", "0", "100", "-1", "275760", "1899", "NaN"]) {
    for (const v of vals) {
      const rest = [y];
      for (let i = 1; i < n; i++) rest.push(i === 1 ? v : i === n - 1 ? v : "1");
      add(`T(()=>new Date(${rest.join(",")}))`);
    }
  }
}
for (const a of nums) add(`T(()=>new Date(${a}))`, `T(()=>new Date(${a},0))`, `T(()=>new Date(2024,${a}))`, `T(()=>new Date(2024,0,${a}))`, `T(()=>new Date(2024,0,1,${a}))`, `T(()=>new Date(2024,0,1,0,${a}))`, `T(()=>new Date(2024,0,1,0,0,${a}))`, `T(()=>new Date(2024,0,1,0,0,0,${a}))`);
add("T(()=>new Date(2024,0,1,0,0,0,0,99))", "T(()=>new Date(2024,0,1,0,0,0,0,99,100))", "T(()=>typeof Date())", "T(()=>Date(2024,0,1).length>20)", "T(()=>Date.length)", "T(()=>new Date().constructor===Date)", "T(()=>typeof Date.now())");

// ---- 5. Tipos de argumento único e duplo.
const types = [
  "'2024-01-15'", "'2024-01-15T10:20:30Z'", "'2024-01-15T10:20:30'", "'2024-01-15T10:20:30+02:00'", "'2024'", "'2024-13'", "'x'", "''", "' '", "'0'", "'12'", "'1e3'", "'Jan 5 2024'", "'2024-02-30'",
  "'+275760-09-13T00:00:00.000Z'", "'-000001-01-01T00:00:00Z'", "'-000000-01-01T00:00:00Z'", "0", "-0", "1e3", "8.64e15", "8.64e15+1", "-8.64e15", "NaN", "Infinity", "1.9", "-1.9", "true", "false", "null", "undefined",
  "new Date(5)", "new Date(NaN)", "new Date(-1)", "[]", "[5]", "[1,2]", "{}", "[0]", "[,]", "new String('2024-01-15')", "new Number(7)", "new Boolean(true)",
  "{valueOf(){return 5}}", "{valueOf(){return 'x'}}", "{valueOf(){return '2024-01-15'}}", "{toString(){return '2024-01-15'}}", "{toString(){return 9},valueOf(){return {}}}",
  "{valueOf(){return {}},toString(){return {}}}", "{valueOf(){throw new RangeError('v')}}", "{toString(){throw new EvalError('t')},valueOf:undefined}",
  "{[Symbol.toPrimitive](h){return h==='default'?11:h==='number'?22:33}}", "{[Symbol.toPrimitive](h){return h}}", "{[Symbol.toPrimitive](h){return {}}}", "{[Symbol.toPrimitive]:1}",
  "{[Symbol.toPrimitive]:null,valueOf(){return 4}}", "{[Symbol.toPrimitive]:undefined,valueOf(){return 4}}", "{[Symbol.toPrimitive](){return 1n}}",
  "{[Symbol.toPrimitive](){return Symbol()}}", "1n", "10n**20n", "-1n", "Symbol()", "Symbol.iterator", "Object(1n)", "Object(Symbol())", "function(){}", "()=>1", "new Proxy(new Date(3),{})",
  "Object.assign(new Date(9),{valueOf(){return 4}})", "Object.assign(new Date(9),{[Symbol.toPrimitive]:undefined})", "Object.assign(new Date(9),{[Symbol.toPrimitive](h){return 77}})",
  "Object.create(Date.prototype)", "new (class extends Date{})(6)",
];
for (const t of types) {
  add(`T(()=>new Date(${t}))`, `T(()=>new Date(${t},1))`, `T(()=>new Date(2024,${t}))`, `T(()=>new Date(${t},1,1))`, `T(()=>new Date(2024,1,${t},1))`);
  add(`T(()=>Date.UTC(${t}))`, `T(()=>Date.UTC(2024,${t}))`, `T(()=>Date.parse(${t}))`, `T(()=>new Date(0).setTime(${t}))`);
}
add("T(()=>{var log=[];var o=n=>({valueOf(){log.push(n);return 1}});new Date(o('y'),o('m'),o('d'),o('h'),o('mi'),o('s'),o('ms'));return log.join()})",
  "T(()=>{var log=[];var o=n=>({valueOf(){log.push(n);return NaN}});new Date(o('y'),o('m'),o('d'));return log.join()})",
  "T(()=>{var log=[];var o=n=>({valueOf(){log.push(n);throw new Error(n)}});try{new Date(o('y'),o('m'))}catch(e){log.push('c'+e.message)}return log.join()})",
  "T(()=>{var log=[];var o=n=>({valueOf(){log.push(n);return 1}});Date.UTC(o('y'),o('m'),o('d'),o('h'),o('mi'),o('s'),o('ms'),o('x'));return log.join()})",
  "T(()=>{var log=[];var d=new Date(0);d.setHours({valueOf(){log.push('h');return 1}},{valueOf(){log.push('m');return 1}},{valueOf(){log.push('s');return 1}},{valueOf(){log.push('ms');return 1}});return log.join()})",
  "T(()=>{var log=[];var d=new Date(NaN);d.setHours({valueOf(){log.push('h');return 1}},{valueOf(){log.push('m');return 1}});return log.join()+S(d)})",
  "T(()=>{var log=[];var d=new Date(NaN);d.setFullYear({valueOf(){log.push('y');return 2000}},{valueOf(){log.push('m');return 1}},{valueOf(){log.push('d');return 2}});return log.join()+S(d)})",
  "T(()=>{var d=new Date(0);d.setMonth({valueOf(){d.setTime(NaN);return 1}});return S(d)})",
  "T(()=>{var d=new Date(0);d.setMonth({valueOf(){d.setTime(86400000*40);return 1}});return S(d)})",
  "T(()=>{var d=new Date(0);d.setHours({valueOf(){d.setTime(NaN);return 1}});return S(d)})",
  "T(()=>{var d=new Date(0);d.setFullYear({valueOf(){d.setTime(NaN);return 2000}});return S(d)})");

// ---- 6. Date.prototype[@@toPrimitive] com hints.
const hints = ["'default'", "'string'", "'number'", "'Default'", "'STRING'", "''", "'x'", "undefined", "null", "1", "{}", "Symbol()", "'number '", "new String('number')", "{toString(){return 'number'}}", "[]", "true", "1n"];
for (const h of hints) {
  add(`T(()=>new Date(86400000)[Symbol.toPrimitive](${h}))`, `T(()=>new Date(NaN)[Symbol.toPrimitive](${h}))`, `T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3},toString(){return 's'}},${h}))`,
    `T(()=>Date.prototype[Symbol.toPrimitive].call(1,${h}))`, `T(()=>Date.prototype[Symbol.toPrimitive].call(undefined,${h}))`, `T(()=>Date.prototype[Symbol.toPrimitive].call('x',${h}))`,
    `T(()=>Date.prototype[Symbol.toPrimitive].call({valueOf:()=>({}),toString:()=>({})},${h}))`, `T(()=>Date.prototype[Symbol.toPrimitive].call(new Proxy({},{get(t,k){return k==='valueOf'?()=>8:()=>'t'}}),${h}))`);
}
add("T(()=>Date.prototype[Symbol.toPrimitive].length)", "T(()=>Date.prototype[Symbol.toPrimitive].name)", "T(()=>Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive).writable)",
  "T(()=>Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive).configurable)", "T(()=>Object.getOwnPropertyDescriptor(Date.prototype,Symbol.toPrimitive).enumerable)",
  "T(()=>Date.prototype[Symbol.toPrimitive].call(new Date(5)))", "T(()=>Date.prototype[Symbol.toPrimitive].call({}))", "T(()=>Date.prototype[Symbol.toPrimitive]())");
for (const v of ["new Date(86400000)", "new Date(NaN)", "new Date(-1)"]) {
  add(`T(()=>${v}+1)`, `T(()=>${v}+'')`, `T(()=>\`\${${v}}\`)`, `T(()=>${v}-0)`, `T(()=>+${v})`, `T(()=>${v}*1)`, `T(()=>String(${v}))`, `T(()=>Number(${v}))`, `T(()=>[${v}]+'')`, `T(()=>JSON.stringify(${v}))`, `T(()=>JSON.stringify({d:${v}}))`,
    `T(()=>${v}==${v}.toString())`, `T(()=>${v}==${v}.getTime())`, `T(()=>${v}==${v})`, `T(()=>${v}<=${v})`, `T(()=>${v}>=${v})`, `T(()=>${v}=== ${v})`, `T(()=>Object.is(${v},${v}))`, `T(()=>${v}+${v})`, `T(()=>${v}-${v})`,
    `T(()=>isNaN(${v}))`, `T(()=>Math.max(${v},0))`, `T(()=>${v}|0)`, `T(()=>${v}&&1)`, `T(()=>!${v})`, `T(()=>typeof (${v}+1))`, `T(()=>typeof (${v}-1))`, `T(()=>BigInt(${v}))`, `T(()=>Symbol.toPrimitive in ${v})`);
}

// ---- 7. toJSON.
for (const o of [
  "{}", "{toISOString(){return 'iso'}}", "{toISOString(){return 1}}", "{toISOString:1}", "{toISOString:null}", "{toISOString:undefined}", "{valueOf(){return 5},toISOString(){return 'v'+this.valueOf()}}",
  "{valueOf(){return NaN},toISOString(){return 'x'}}", "{valueOf(){return Infinity},toISOString(){return 'x'}}", "{valueOf(){return 'str'},toISOString(){return 'x'}}", "{valueOf(){return {}},toISOString(){return 'x'},toString(){return {}}}",
  "{[Symbol.toPrimitive](h){return h==='number'?Infinity:1},toISOString(){return 'tp'}}", "{[Symbol.toPrimitive](h){return h==='number'?4:'s'},toISOString(){return 'tp'}}", "{toISOString(){throw new RangeError('boom')}}",
  "{valueOf(){throw new EvalError('vo')},toISOString(){return 1}}", "1", "'s'", "true", "null", "undefined", "Symbol()", "1n", "[]", "()=>1", "new Date(0)", "new Date(NaN)", "new Number(NaN)", "new Number(3)", "Object(1n)",
  "Object.assign(new Date(0),{toISOString(){return 'custom'}})", "Object.assign(new Date(NaN),{toISOString(){return 'custom'}})", "new (class extends Date{toISOString(){return 'sub'}})(0)", "new Proxy({toISOString(){return 'px'}},{})",
  "new Proxy(new Date(0),{get(t,k){return k==='toISOString'?()=>'trap':Reflect.get(t,k)}})", "Object.create({toISOString(){return 'inh'}})", "{get toISOString(){return ()=>'getter'}}", "{get toISOString(){throw new TypeError('g')}}",
]) {
  add(`T(()=>Date.prototype.toJSON.call(${o}))`, `T(()=>Date.prototype.toJSON.call(${o},'key'))`, `T(()=>JSON.stringify({k:${o}}))`, `T(()=>JSON.stringify([${o}]))`);
}
add("T(()=>Date.prototype.toJSON.length)", "T(()=>Date.prototype.toJSON.name)", "T(()=>Date.prototype.toJSON())", "T(()=>new Date(0).toJSON.call(null))", "T(()=>Date.prototype.toJSON.call({toISOString(){return this===globalThis}}))",
  "T(()=>{var r;Date.prototype.toJSON.call({toISOString(){r=arguments.length}});return r})", "T(()=>JSON.stringify(new Date(8.64e15)))", "T(()=>JSON.stringify(new Date(8.64e15+1)))", "T(()=>JSON.stringify(new Date(-62198755200000)))", "T(()=>JSON.stringify({a:new Date(NaN),b:new Date(0)}))",
  "T(()=>JSON.stringify(new Date(0),(k,v)=>typeof v))", "T(()=>JSON.stringify({d:new Date(0)},(k,v)=>k==='d'?typeof v:v))", "T(()=>JSON.stringify({d:new Date(0)},function(k,v){return k==='d'?typeof this[k]:v}))",
  "T(()=>JSON.parse(JSON.stringify(new Date(5))))", "T(()=>JSON.parse('\"1970-01-01T00:00:00.005Z\"',(k,v)=>new Date(v)))");

// ---- 8. Subclasses.
add(
  "T(()=>{class D extends Date{};var d=new D(5);return S(d)+(d instanceof D)+(d instanceof Date)+d.getTime()+Object.prototype.toString.call(d)})",
  "T(()=>{class D extends Date{};return S(new D(2024,0,31))})", "T(()=>{class D extends Date{};return S(new D('2024-01-15'))})", "T(()=>{class D extends Date{};return S(new D(NaN))})", "T(()=>{class D extends Date{};return typeof D()})",
  "T(()=>{class D extends Date{};return D.UTC(2024)+D.now().constructor.name+D.parse('1970')})", "T(()=>{class D extends Date{constructor(){super(7)}};return S(new D())+new D().getTime()})",
  "T(()=>{class D extends Date{constructor(...a){super(...a);this.x=1}};var d=new D(1,2,3);return S(d)+d.x})", "T(()=>{class D extends Date{constructor(){}};new D()})", "T(()=>{class D extends Date{constructor(){super();return 1}};return typeof new D()})",
  "T(()=>{class D extends Date{valueOf(){return 42}};var d=new D(5);return d+1+' '+(d-1)+' '+(d<50)+' '+d.getTime()+' '+JSON.stringify(d)+' '+String(d)})",
  "T(()=>{class D extends Date{toString(){return 'ts'}};var d=new D(5);return d+1+' '+`${d}`+' '+d-1+' '+String(d)+' '+JSON.stringify(d)})",
  "T(()=>{class D extends Date{[Symbol.toPrimitive](h){return h}};var d=new D(5);return d+1+' '+`${d}`+' '+(d-1)+' '+(+d)})",
  "T(()=>{class D extends Date{getTime(){return 1}};var d=new D(5);return d.valueOf()+' '+d.getTime()+' '+(+d)+' '+d.toISOString()})", "T(()=>{class D extends Date{toISOString(){return 'x'}};return JSON.stringify(new D(5))})",
  "T(()=>{class D extends Date{};D.prototype.getTime=()=>9;return new D(5).toJSON()+new D(NaN).toJSON()})", "T(()=>{class D extends Date{};return Object.getPrototypeOf(D)===Date&&D.length+D.name})",
  "T(()=>{function F(){};F.prototype=Date.prototype;return S(Reflect.construct(Date,[5],F))+(Reflect.construct(Date,[5],F) instanceof F)})",
  "T(()=>{var d=Reflect.construct(Date,[5],Object);return Object.getPrototypeOf(d)===Object.prototype})", "T(()=>{var d=Reflect.construct(Date,[5],Array);return Object.getPrototypeOf(d)===Array.prototype})",
  "T(()=>{var d=Reflect.construct(Date,[5],Object);return Date.prototype.getTime.call(d)})", "T(()=>{var nt=function(){}.bind();nt.prototype=null;var d=Reflect.construct(Date,[5],nt);return Object.getPrototypeOf(d)===Object.prototype})",
  "T(()=>{var d=Object.create(Date.prototype);return d.getTime()})", "T(()=>{var d=Object.setPrototypeOf(new Date(5),null);return Date.prototype.getTime.call(d)})", "T(()=>{var d=new Date(5);Object.setPrototypeOf(d,{});return Date.prototype.getTime.call(d)+typeof d.getTime})",
  "T(()=>Date.prototype.constructor===Date)", "T(()=>Object.prototype.toString.call(Date.prototype))", "T(()=>Object.prototype.toString.call(new Date(0)))", "T(()=>Object.prototype.toString.call(Date))", "T(()=>{var d=new Date(0);d[Symbol.toStringTag]='X';return Object.prototype.toString.call(d)})",
  "T(()=>Object.getOwnPropertyNames(new Date(0)).length)", "T(()=>Object.keys(new Date(0)).length)", "T(()=>Object.getPrototypeOf(Date.prototype)===Object.prototype)", "T(()=>{var d=new Date(0);d.x=1;return JSON.stringify(d)+Object.keys(d)})",
  "T(()=>{var d=new Date(0);return S(Object.assign(d,{y:2}))})", "T(()=>Object.getOwnPropertyDescriptor(Date,'prototype').writable)", "T(()=>Date.UTC.length+Date.parse.length+Date.now.length)", "T(()=>Date.name+Date.length)",
);

// ---- 9. Aritmética e comparação entre Dates.
const ds = ["new Date(0)", "new Date(1)", "new Date(86400000)", "new Date(-1)", "new Date(NaN)", "new Date(8.64e15)", "new Date(2024,0,1)"];
const ops = ["+", "-", "*", "/", "%", "<", ">", "<=", ">=", "==", "!=", "===", "!==", "&&", "||", "??", "|", "**"];
for (const a of ds) for (const b of ds) for (const op of ops) add(`T(()=>${a}${op}${b})`);
const others = ["1", "'1'", "''", "null", "undefined", "true", "NaN", "'1970-01-01T00:00:00.000Z'", "0", "'x'", "[]", "{}", "1n", "new Date(0).toString()", "new Date(0).getTime()"];
for (const a of ["new Date(0)", "new Date(5)", "new Date(NaN)"]) for (const b of others) for (const op of ["+", "-", "<", ">", "==", "!=", "<=", ">="]) add(`T(()=>${a}${op}${b})`, `T(()=>${b}${op}${a})`);
add("T(()=>new Date(0)==new Date(0))", "T(()=>new Date(0)<new Date(1))", "T(()=>{var a=new Date(0);return a==a})", "T(()=>[new Date(3),new Date(1),new Date(2)].sort((a,b)=>a-b).map(d=>+d).join())", "T(()=>[new Date(3),new Date(1),new Date(2)].sort().map(d=>+d).join())",
  "T(()=>Math.max(new Date(3),new Date(9)))", "T(()=>Math.min(new Date(3),new Date(NaN)))", "T(()=>new Date(new Date(5)).getTime())", "T(()=>new Date(new Date(NaN)).getTime())", "T(()=>{var d=new Date(5);var c=new Date(d);c.setTime(1);return d.getTime()})",
  "T(()=>Number(new Date(5))+Number(new Date(6)))", "T(()=>new Date(5)-'1')", "T(()=>new Date(5)+'1')", "T(()=>(new Date(5)-new Date(3))/1000)", "T(()=>new Date(+new Date(5)+86400000).getTime())", "T(()=>new Date(2024,0,31)-new Date(2024,0,1))",
  "T(()=>Math.round((new Date(2024,2,1)-new Date(2024,1,1))/864e5))", "T(()=>new Date(2024,0,1)<new Date(2024,0,2)?'lt':'ge')", "T(()=>new Date(NaN)<new Date(0)||new Date(NaN)>new Date(0)||new Date(NaN)==new Date(NaN))",
  "T(()=>`${new Date(0)}`===new Date(0).toString())", "T(()=>new Date(0)+new Date(0)==new Date(0).toString()+new Date(0).toString())", "T(()=>new Date(0)=='Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)')", "T(()=>new Date(0)==0)", "T(()=>new Date(0)=='0')");

// ---- 10. toISOString, RangeError e anos extremos.
const extreme = [
  "0", "-1", "8.64e15", "8.64e15+1", "-8.64e15", "-8.64e15-1", "NaN", "Infinity", "-Infinity", "-62198755200000", "-62198755200001", "-62167219200000", "-62167219200001", "253402300799999", "253402300800000", "-62135596800000", "8.639999999999999e15",
  "-8.639999999999999e15", "9007199254740993", "-1e-7", "1e-7", "0.9", "-0.9", "1.9999", "-1.0001", "951782400000", "-30610224000000", "-30610224000001", "1e300", "-0",
];
for (const e of extreme) {
  for (const m of ["toISOString", "toJSON", "toString", "toUTCString", "toGMTString", "toDateString", "toTimeString", "toLocaleString", "toLocaleDateString", "toLocaleTimeString", "getTime", "getFullYear", "getUTCFullYear", "getYear", "getDay", "getTimezoneOffset"]) add(`T(()=>new Date(${e}).${m}())`);
  add(`T(()=>String(new Date(${e})))`, `T(()=>new Date(${e}).toISOString().length)`, `T(()=>Date.prototype.toISOString.call(new Date(${e})))`, `T(()=>new Date(new Date(${e}).toISOString()).getTime())`, `T(()=>Date.parse(new Date(${e}).toString()))`,
    `T(()=>Date.parse(new Date(${e}).toUTCString()))`, `T(()=>Date.UTC(new Date(${e}).getUTCFullYear(),0))`);
}
for (const y of [271820, 271821, 271822, 275759, 275760, 275761, -271820, -271821, -271822, -271823, -275760, -275761, 99999, -99999, 100000, -100000, 10000, 9999, -1, 0, 1]) {
  for (const [m, d] of [[0, 1], [11, 31], [8, 13], [3, 20], [8, 14], [0, 0]]) {
    add(`T(()=>new Date(Date.UTC(${y},${m},${d})).toISOString())`, `T(()=>Date.UTC(${y},${m},${d}))`, `T(()=>new Date(${y},${m},${d}).getTime())`, `T(()=>new Date(${y},${m},${d}).getFullYear())`);
  }
  add(`T(()=>{var d=new Date(0);d.setFullYear(${y});return S(d)+" "+d.getTime()})`, `T(()=>new Date('${y < 0 ? "-" : "+"}${String(Math.abs(y)).padStart(6, "0")}-01-01T00:00:00Z').getTime())`, `T(()=>new Date('${y < 0 ? "-" : "+"}${String(Math.abs(y)).padStart(6, "0")}-01-01T00:00:00Z').toISOString())`,
    `T(()=>new Date(Date.UTC(${y},0)).toUTCString())`, `T(()=>new Date(Date.UTC(${y},0)).toString())`, `T(()=>new Date(Date.UTC(${y},0)).toDateString())`, `T(()=>new Date(Date.UTC(${y},0)).toJSON())`);
}
add("T(()=>new Date(8.64e15).toISOString())", "T(()=>new Date(-8.64e15).toISOString())", "T(()=>new Date(8.64e15).getUTCFullYear())", "T(()=>new Date(-8.64e15).getUTCFullYear())", "T(()=>new Date(8.64e15).getUTCDay())", "T(()=>new Date(-8.64e15).getUTCDay())",
  "T(()=>new Date(8.64e15).setMilliseconds(1))", "T(()=>new Date(8.64e15-1).setMilliseconds(1000))", "T(()=>new Date(-8.64e15).setMilliseconds(-1))", "T(()=>new Date(8.64e15).setUTCDate(14))", "T(()=>new Date(8.64e15).setFullYear(275760))",
  "T(()=>new Date(8.64e15).setHours(-24))", "T(()=>new Date(0).setUTCFullYear(275760,8,13))", "T(()=>new Date(0).setUTCFullYear(275760,8,14))", "T(()=>new Date(0).setUTCFullYear(-271821,3,20))", "T(()=>new Date(0).setUTCFullYear(-271821,3,19))",
  "T(()=>new Date(0).setUTCFullYear(-271821,3,20)===-8.64e15)", "T(()=>new Date(0).setUTCHours(24*8.64e7))", "T(()=>new Date(0).setUTCHours(0,0,0,8.64e15))", "T(()=>new Date(0).setUTCHours(0,0,0,8.64e15+1))", "T(()=>new Date(1).setUTCMilliseconds(8.64e15))");

// ---- 11. Anos 0..99 no construtor contra Date.UTC e setters.
for (let y = 0; y <= 101; y++) {
  add(`T(()=>new Date(${y},0).getFullYear()+' '+Date.UTC(${y},0)+' '+new Date(${y},0).getTime())`);
  add(`T(()=>S(new Date(${y},11,31,23,59,59,999))+' '+new Date(${y},0,1).getYear())`);
  add(`T(()=>{var d=new Date(2000,5,15);d.setFullYear(${y});return d.getFullYear()+' '+d.getTime()})`);
  add(`T(()=>{var d=new Date(2000,5,15);d.setYear(${y});return d.getFullYear()+' '+d.getYear()})`);
  add(`T(()=>S(new Date(Date.UTC(${y},1,29)))+' '+new Date(${y},1,29).getDate()`+`)`);
  add(`T(()=>new Date('${y}').getTime())+' '+T(()=>Date.parse('${y}-06-15'))+' '+T(()=>Date.parse('6/15/${y}'))`);
}
for (const y of ["-1", "-50", "-99", "-100", "99.9", "-0.9", "0.9", "'50'", "'0'", "50.5", "NaN", "undefined", "null", "'x'", "100.1", "1e3", "true", "{valueOf(){return 55}}"]) {
  add(`T(()=>new Date(${y},0).getFullYear())`, `T(()=>Date.UTC(${y},0))`, `T(()=>new Date(${y}).getTime())`, `T(()=>new Date(${y},0,1,0,0,0,0).getTime())`, `T(()=>Date.UTC(${y}))`, `T(()=>new Date(${y},undefined).getTime())`, `T(()=>Date.UTC(${y},undefined))`);
}
add("T(()=>Date.UTC())", "T(()=>Date.UTC(undefined))", "T(()=>Date.UTC(2024))", "T(()=>Date.UTC(2024,0))", "T(()=>Date.UTC(2024,undefined))", "T(()=>Date.UTC(NaN))", "T(()=>new Date(undefined).getTime())", "T(()=>new Date(null).getTime())", "T(()=>new Date(2024,undefined).getTime())");

// ---- Execução.
const baseSources = [];
baseSources.push(...knownPrograms("date_legacy_bun.tsv", ["date_utc_bun.tsv", "date_core_bun.tsv"]));
const baseText = baseSources.join("\n\u0000\n");
const seen = new Set();
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
let dup = 0;
for (const expr of unique) {
  // A expressão sozinha já aparecer num golden existente conta como repetida.
  if (expr.length > 24 && baseText.includes(expr)) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  let result;
  try {
    // Processo fresco por programa e TZ=UTC fixo no filho, sem depender do ambiente de quem gera.
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, env: { ...process.env, TZ: "UTC" } });
    if (child.status !== 0) throw new Error(child.stderr || "filho falhou");
    result = decodeResult(child.stdout);
    if (result === null) throw new Error("filho sem resultado");
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr).slice(0, 160) + " " + e + "\n");
    dropped++;
    continue;
  }
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
    dropped++;
    process.stderr.write("caminho ou marca no resultado: " + JSON.stringify(expr).slice(0, 160) + "\n");
    continue;
  }
  // Instante atual muda a cada execução: fora do golden.
  if (/Date\.now\(\)|new Date\(\)\.constructor/.test(expr) && !/typeof|constructor===Date/.test(expr)) { dropped++; continue; }
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("date_legacy", rows));
