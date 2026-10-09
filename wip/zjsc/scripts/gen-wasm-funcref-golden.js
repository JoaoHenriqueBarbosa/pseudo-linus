// Gera tests/golden/wasm_funcref_bun.tsv: funcref e externref através da API JS do WebAssembly, medidos no bun.
// Cobre Table de funcref com exports de várias instâncias, identidade (===) de Table.get, Table.set com função JS pura,
// call_indirect pela Table compartilhada com assinatura certa e errada, Table.grow com valor inicial, Global funcref e
// externref (mutável e imutável), import de função exportada de outra instância e reexport, ref.func de import, Table de
// externref com objetos, primitivos, undefined e null, e as mensagens exatas dos erros.
// Os módulos saem de um mini-assembler JS (abaixo) como bytes literais `new Uint8Array([...])`, sem arquivos.
// Cada programa roda num bun filho novo (no máximo 6 em paralelo, timeout de 8 s), sem APIs de host, e grava o texto em
// `globalThis.R`. Programas cuja fonte já aparece nos goldens wasm_*_bun.tsv são descartados. Uso:
//   bun scripts/gen-wasm-funcref-golden.js > tests/golden/wasm_funcref_bun.tsv
const fs = require("fs");
const { spawn } = require("child_process");
const { emitFactored, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

// ---- mini-assembler.
const u = (n) => { const r = []; do { let b = n & 127; n >>>= 7; if (n) b |= 128; r.push(b); } while (n); return r; };
const s = (n) => { const r = []; for (;;) { const b = n & 127; n >>= 7; if ((n === 0 && !(b & 64)) || (n === -1 && (b & 64))) { r.push(b); return r; } r.push(b | 128); } };
const st = (x) => [...u(x.length), ...[...x].map((c) => c.charCodeAt(0))];
const vec = (a) => [...u(a.length), ...a.flat()];
const sec = (id, b) => [id, ...u(b.length), ...b];
const M = (...x) => [0, 97, 115, 109, 1, 0, 0, 0, ...x.flat()];
const ft = (p, r) => [0x60, ...vec(p.map((x) => [x])), ...vec(r.map((x) => [x]))];
const Fn = (code) => { const b = [0, ...code, 0x0b]; return [...u(b.length), ...b]; };
const I32 = 0x7f, FR = 0x70, ER = 0x6f;
const TY = (...f) => sec(1, vec(f));
const FS = (...i) => sec(3, vec(i.map((x) => [x])));
const CS = (...f) => sec(10, vec(f));
const ES = (...e) => sec(7, vec(e.map(([n, k, i]) => [...st(n), k, ...u(i)])));
const lim = (min, max) => (max === null ? [0, ...u(min)] : [1, ...u(min), ...u(max)]);

// Provider: f ()->i32 devolve k, g (i32)->i32 soma k, rf ()->funcref faz ref.func 0, t tabela funcref 4..6 com [f,g,f,null].
const mkProv = (k) => M(TY(ft([], [I32]), ft([I32], [I32]), ft([], [FR])), FS(0, 1, 2), sec(4, vec([[FR, ...lim(4, 6)]])),
  ES(["f", 0, 0], ["g", 0, 1], ["rf", 0, 2], ["t", 1, 0]),
  sec(9, vec([[0, 0x41, ...s(0), 0x0b, ...vec([[0], [1], [0]])]])),
  CS(Fn([0x41, ...s(k)]), Fn([0x20, 0, 0x41, ...s(k), 0x6a]), Fn([0xd2, 0])));
// Caller: importa a tabela env.t e expõe call_indirect (c0 sem argumento, c1 com um), table.get/set/grow/size.
const mkCaller = (min, max) => M(TY(ft([], [I32]), ft([I32], [I32]), ft([I32, I32], [I32]), ft([I32], [FR]), ft([I32, FR], []), ft([FR, I32], [I32])),
  sec(2, vec([[...st("env"), ...st("t"), 1, FR, ...lim(min, max)]])), FS(1, 2, 3, 4, 5, 0),
  ES(["c0", 0, 0], ["c1", 0, 1], ["c2", 0, 2], ["c3", 0, 3], ["c4", 0, 4], ["c5", 0, 5], ["t", 1, 0]),
  CS(Fn([0x20, 0, 0x11, 0, 0]), Fn([0x20, 1, 0x20, 0, 0x11, 1, 0]), Fn([0x20, 0, 0x25, 0]), Fn([0x20, 0, 0x20, 1, 0x26, 0]),
    Fn([0x20, 0, 0x20, 1, 0xfc, 0x0f, 0]), Fn([0xfc, 0x10, 0])));
// FImp: importa env.f com a assinatura sig, reexporta como r; rf faz ref.func do import; call chama (só sig 0).
const SIGS = [ft([], [I32]), ft([I32], [I32]), ft([I32], [])];
const mkFimp = (sig) => M(TY(SIGS[sig], ft([], [FR]), ft([], [I32])), sec(2, vec([[...st("env"), ...st("f"), 0, 0]])),
  FS(...(sig === 0 ? [1, 2] : [1])), ES(["r", 0, 0], ["rf", 0, 1], ...(sig === 0 ? [["call", 0, 2]] : [])),
  CS(Fn([0xd2, 0]), ...(sig === 0 ? [Fn([0x10, 0])] : [])));
// Glob: globais funcref (imutável com ref.func, mutável null) e externref (mutável null, imutável null) com getters e setters.
const mkGlob = () => M(TY(ft([], [I32]), ft([], [FR]), ft([FR], []), ft([], [ER]), ft([ER], []), ft([ER], [ER]), ft([FR], [FR])),
  FS(0, 1, 2, 3, 4, 5, 6, 1, 3),
  sec(6, vec([[FR, 0, 0xd2, 0, 0x0b], [FR, 1, 0xd0, FR, 0x0b], [ER, 1, 0xd0, ER, 0x0b], [ER, 0, 0xd0, ER, 0x0b]])),
  ES(["f", 0, 0], ["gi", 3, 0], ["gm", 3, 1], ["ge", 3, 2], ["gx", 3, 3], ["get_gm", 0, 1], ["set_gm", 0, 2], ["get_ge", 0, 3], ["set_ge", 0, 4],
    ["id", 0, 5], ["idf", 0, 6], ["get_gi", 0, 7], ["get_gx", 0, 8]),
  CS(Fn([0x41, ...s(99)]), Fn([0x23, 1]), Fn([0x20, 0, 0x24, 1]), Fn([0x23, 2]), Fn([0x20, 0, 0x24, 2]), Fn([0x20, 0]), Fn([0x20, 0]), Fn([0x23, 0]), Fn([0x23, 3])));
// Gimp: importa env.g global (tipo, mutabilidade) e expõe get, set (se mutável) e reexporta como g.
const mkGimp = (type, mut) => M(TY(ft([], [type]), ft([type], [])), sec(2, vec([[...st("env"), ...st("g"), 3, type, mut]])),
  FS(0, ...(mut ? [1] : [])), ES(["get", 0, 0], ...(mut ? [["set", 0, 1]] : []), ["g", 3, 0]),
  CS(Fn([0x23, 0]), ...(mut ? [Fn([0x20, 0, 0x24, 0])] : [])));
// Ext: tabela externref (3, sem máximo) com id, get, set, grow, size, isnull.
const mkExt = () => M(TY(ft([ER], [ER]), ft([I32], [ER]), ft([I32, ER], []), ft([ER, I32], [I32]), ft([], [I32]), ft([ER], [I32])),
  FS(0, 1, 2, 3, 4, 5), sec(4, vec([[ER, ...lim(3, null)]])),
  ES(["id", 0, 0], ["get", 0, 1], ["set", 0, 2], ["grow", 0, 3], ["size", 0, 4], ["isnull", 0, 5], ["t", 1, 0]),
  CS(Fn([0x20, 0]), Fn([0x20, 0, 0x25, 0]), Fn([0x20, 0, 0x20, 1, 0x26, 0]), Fn([0x20, 0, 0x20, 1, 0xfc, 0x0f, 0]), Fn([0xfc, 0x10, 0]), Fn([0x20, 0, 0xd1])));

const lit = (bytes) => `new Uint8Array([${bytes.join(",")}])`;
const PRELUDE =
  '"use strict";\n' +
  `var PA=${lit(mkProv(10))},PB=${lit(mkProv(20))},CL=${lit(mkCaller(4, 6))},FI0=${lit(mkFimp(0))},GL=${lit(mkGlob())},EX=${lit(mkExt())};\n` +
  'var D=v=>{var t=typeof v;if(v===null)return"null";if(t==="function")return"fn:"+v.name+"/"+v.length;if(t==="object")return Object.prototype.toString.call(v);if(t==="bigint")return v+"n";if(t==="symbol")return"sym";if(Object.is(v,-0))return"-0";return t+":"+String(v)};\n' +
  'var T=f=>{try{var v=f();return typeof v==="string"?v:D(v)}catch(e){return e.name+": "+e.message}};\n' +
  'var inst=(m,o)=>new WebAssembly.Instance(new WebAssembly.Module(m),o).exports;\n' +
  'var Q=t=>T(()=>{var o=[];for(var k=0;k<t.length;k++)o.push(D(t.get(k)));return t.length+"["+o.join()+"]"});\n' +
  'var Z=(r,v)=>D(r)+(Object.is(r,v)?"=":"!");\n' +
  'var W=()=>{var a=inst(PA),b=inst(PB),c=inst(CL,{env:{t:a.t}}),g=inst(GL),e=inst(EX),j=function(){return 7},fi=inst(FI0,{env:{f:a.f}});return{a,b,c,g,e,j,fi}};\n';

// ---- casos.
const exprs = [];
const add = (expr) => exprs.push(expr);
const VALS = ["undefined", "null", "0", "-0", "1", "1.5", "NaN", "Infinity", "'s'", "''", "true", "false", "1n", "Symbol.iterator", "{}", "[]",
  "function(){}", "()=>1", "class{}", "Math.abs", "new Number(1)", "Object.create(null)", "new Proxy({},{})", "new Proxy(function(){},{})",
  "a.f.bind(null)", "a.f", "b.g", "a.t", "g.id", "g.idf", "c.c2", "j", "async function(){}", "function*(){}", "Object", "e.t", "a.rf()", "fi.r", "g.gm", "WebAssembly"];
const IDX = ["0", "1", "2", "3", "4", "5", "-1", "1.5", "'1'", "'x'", "undefined", "null", "NaN", "Infinity", "2**32", "2**32+1", "-0", "true", "{}", "1n", "6", "7"];

// 1. call_indirect pela tabela compartilhada.
const setups = ["0", "a.t.set(0,b.f)", "a.t.set(1,b.g)", "a.t.set(0,b.g)", "a.t.set(2,b.f)", "a.t.set(3,b.f)", "a.t.set(3,null)", "a.t.set(0,null)",
  "a.t.grow(1,b.f)", "a.t.grow(2)", "a.t.set(1,a.f)", "a.t.set(0,fi.r)", "a.t.set(0,fi.rf())", "a.t.set(1,c.c0)", "a.t.set(0,g.f)", "a.t.set(2,b.rf())"];
for (const sp of setups) {
  for (let i = -1; i <= 7; i++) {
    add(`T(()=>{${sp};return 0})+"|"+T(()=>c.c0(${i}))+"|"+Q(a.t)`);
    for (const x of [0, 5]) add(`T(()=>{${sp};return 0})+"|"+T(()=>c.c1(${i},${x}))+"|"+T(()=>c.c5())`);
  }
}
for (let i = 0; i <= 5; i++) {
  add(`T(()=>{var d=inst(CL,{env:{t:b.t}});return d.c0(${i})})+"|"+T(()=>{var d=inst(CL,{env:{t:b.t}});return d.c1(${i},3)})`);
  add(`T(()=>{a.t.set(${i},b.t.get(0));return c.c0(${i})})+"|"+Q(a.t)`);
  add(`T(()=>{b.t.set(${i},a.t.get(1));return inst(CL,{env:{t:b.t}}).c1(${i},4)})`);
}

// 2. identidade.
const ITEMS = ["a.f", "b.f", "a.g", "a.t.get(0)", "a.t.get(1)", "a.t.get(2)", "a.t.get(3)", "b.t.get(0)", "c.c2(0)", "c.c2(1)", "c.c2(2)", "a.rf()", "b.rf()",
  "c.t", "a.t", "b.t", "fi.r", "fi.rf()", "g.f", "g.get_gi()", "g.get_gm()"];
for (const x of ITEMS) for (const y of ITEMS) add(`T(()=>[${x}===${y},Object.is(${x},${y})].join())`);
for (const x of ITEMS) add(`T(()=>{var v=${x};return [typeof v,v===v,v&&v.name,v&&v.length].join()})`);

// 3. Table.get e Table.set com índices.
for (const i of IDX) {
  for (const op of [`a.t.get(${i})`, `a.t.set(${i},a.f)`, `a.t.set(${i},null)`, `a.t.set(${i},undefined)`, `a.t.set(${i},b.g)`, `a.t.set(${i},function(){})`,
    `a.t.set(${i})`, `a.t.set(${i},b.t.get(0))`, `a.t.grow(${i})`, `a.t.grow(1,${i})`]) {
    add(`T(()=>${op})+"|"+Q(a.t)`);
  }
}
add("T(()=>a.t.get())"); add("T(()=>a.t.set())"); add("T(()=>a.t.grow())");
add("T(()=>WebAssembly.Table.prototype.get.call({},0))"); add("T(()=>WebAssembly.Table.prototype.set.call(a.t,0,a.f,1))");
add("T(()=>WebAssembly.Table.prototype.get.call(e.t,0))"); add("T(()=>WebAssembly.Table.prototype.length)");
add("T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Table.prototype,'length').get.call(a.t))");
add("T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Table.prototype,'length').get.call({}))");

// 4. grade de valores.
for (const v of VALS) {
  add(`T(()=>a.t.set(1,${v}))+"|"+Q(a.t)`);
  add(`T(()=>a.t.set(3,${v}))+"|"+Q(a.t)`);
  add(`T(()=>a.t.grow(1,${v}))+"|"+Q(a.t)`);
  add(`T(()=>new WebAssembly.Table({element:'anyfunc',initial:2},${v}))+"|"+T(()=>Q(new WebAssembly.Table({element:'anyfunc',initial:2},${v})))`);
  add(`T(()=>Q(new WebAssembly.Table({element:'funcref',initial:2,maximum:3},${v})))`);
  add(`T(()=>Q(new WebAssembly.Table({element:'externref',initial:2},${v})))`);
  add(`T(()=>{var x=new WebAssembly.Global({value:'anyfunc',mutable:true},${v});return Z(x.value,${v})})`);
  add(`T(()=>{var x=new WebAssembly.Global({value:'funcref',mutable:false},${v});return Z(x.value,${v})+"|"+Z(x.valueOf(),${v})})`);
  add(`T(()=>{var x=new WebAssembly.Global({value:'anyfunc',mutable:true});x.value=${v};return Z(x.value,${v})})`);
  add(`T(()=>{var x=new WebAssembly.Global({value:'externref',mutable:true},${v});return Z(x.value,${v})})`);
  add(`T(()=>{var x=new WebAssembly.Global({value:'externref',mutable:false},${v});return Z(x.value,${v})})`);
  add(`T(()=>{var x=new WebAssembly.Global({value:'externref',mutable:true});x.value=${v};return Z(x.value,${v})})`);
  add(`T(()=>{g.set_gm(${v});return Z(g.get_gm(),${v})})+"|"+T(()=>Z(g.gm.value,${v}))`);
  add(`T(()=>{g.gm.value=${v};return Z(g.get_gm(),${v})})`);
  add(`T(()=>Z(g.idf(${v}),${v}))`);
  add(`T(()=>{g.set_ge(${v});return Z(g.get_ge(),${v})})+"|"+T(()=>Z(g.ge.value,${v}))`);
  add(`T(()=>{g.ge.value=${v};return Z(g.get_ge(),${v})})`);
  add(`T(()=>{g.gi.value=${v};return 0})+"|"+T(()=>{g.gx.value=${v};return 0})`);
  add(`T(()=>Z(g.id(${v}),${v}))`);
  add(`T(()=>e.isnull(${v}))`);
  add(`T(()=>{e.set(0,${v});return Z(e.get(0),${v})+"|"+Z(e.t.get(0),${v})})`);
  add(`T(()=>{e.t.set(1,${v});return Z(e.t.get(1),${v})+"|"+Z(e.get(1),${v})})`);
  add(`T(()=>{var n=e.grow(${v},2);return n+"|"+Z(e.t.get(3),${v})+"|"+Z(e.get(4),${v})+"|"+e.size()})`);
  add(`T(()=>{var n=e.t.grow(1,${v});return n+"|"+Z(e.t.get(3),${v})+"|"+e.size()})`);
  add(`T(()=>{c.c3(0,${v});return Q(a.t)})`);
  add(`T(()=>{c.c3(3,${v});return Q(a.t)})`);
  add(`T(()=>c.c4(${v},1))+"|"+Q(a.t)`);
  add(`T(()=>{var i=inst(${lit(mkGimp(FR, 1))},{env:{g:new WebAssembly.Global({value:'anyfunc',mutable:true},${v})}});return Z(i.get(),${v})})`);
  add(`T(()=>{var i=inst(${lit(mkGimp(ER, 1))},{env:{g:new WebAssembly.Global({value:'externref',mutable:true},${v})}});i.set(${v});return Z(i.get(),${v})+"|"+Z(i.g.value,${v})})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${v}}});return [Z(i.r,${v}),Z(i.rf(),${v})].join()})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${v}}});a.t.set(0,i.r);return c.c0(0)})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${v}}});a.t.set(0,i.rf());return Z(a.t.get(0),i.r)})`);
}

// 5. Table.grow, construtor e Global.
for (const n of ["0", "1", "2", "3", "4", "100", "-1", "2**32", "1.5", "'1'", "undefined", "null", "{}", "4294967295", "10000000"]) {
  for (const init of ["", ",undefined", ",null", ",a.f", ",b.g", ",function(){}", ",5", ",a.rf()"]) {
    add(`T(()=>a.t.grow(${n}${init}))+"|"+Q(a.t)`);
    add(`T(()=>{var t=new WebAssembly.Table({element:'externref',initial:1,maximum:4});var r=t.grow(${n}${init});return r+"|"+Q(t)})`);
    add(`T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});var r=t.grow(${n}${init});return r+"|"+Q(t)})`);
  }
}
for (const el of ["'anyfunc'", "'funcref'", "'externref'", "'i32'", "''", "undefined", "'AnyFunc'", "'anyref'", "'eqref'", "1", "null", "'externref '"]) {
  for (const ini of ["0", "1", "2", "'2'", "-1", "65536", "10000000", "10000001", "undefined", "null", "2**32", "1.5"]) {
    for (const mx of ["undefined", "0", "1", "2", "3", "1e10", "-1", "2**32"]) {
      add(`T(()=>Q(new WebAssembly.Table({element:${el},initial:${ini},maximum:${mx}})))`);
    }
  }
}
for (const ty of ["'i32'", "'anyfunc'", "'funcref'", "'externref'", "'i64'", "'f32'", "'v128'", "'x'", "undefined"]) {
  for (const mu of ["true", "false", "undefined", "1", "''"]) {
    for (const init of ["", ",undefined", ",null", ",a.f", ",0", ",'x'", ",function(){}", ",b.t.get(0)"]) {
      add(`T(()=>{var x=new WebAssembly.Global({value:${ty},mutable:${mu}}${init});var o=[D(x.value),typeof x.valueOf()];try{x.value=a.f;o.push("set ok",D(x.value))}catch(er){o.push(er.name+": "+er.message)}return o.join("|")})`);
    }
  }
}

// 6. imports e reexports de função.
const FIMPS = ["a.f", "b.f", "a.g", "a.rf", "c.c0", "c.c5", "j", "Math.abs", "a.t", "null", "undefined", "5", "'x'", "class{}", "async function(){}",
  "a.f.bind()", "new Proxy(a.f,{})", "g.id", "fi.r", "fi.rf", "{}", "e.id"];
for (const f of FIMPS) {
  add(`T(()=>{var i=inst(FI0,{env:{f:${f}}});return [Z(i.r,${f}),Z(i.rf(),${f}),i.r===i.rf(),i.rf()===i.rf(),D(i.r)].join()})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${f}}});return i.call()})`);
  add(`T(()=>{var i=inst(${lit(mkFimp(1))},{env:{f:${f}}});return [Z(i.r,${f}),Z(i.rf(),${f})].join()})`);
  add(`T(()=>{var i=inst(${lit(mkFimp(2))},{env:{f:${f}}});return [Z(i.r,${f}),Z(i.rf(),${f})].join()})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${f}}});var k=inst(FI0,{env:{f:i.r}});var l=inst(FI0,{env:{f:k.r}});return [l.r===i.r,l.r===${f},l.rf()===i.rf(),D(l.r)].join()})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${f}}});a.t.set(1,i.r);return Q(a.t)})`);
  add(`T(()=>{var i=inst(FI0,{env:{f:${f}}});return [Object.getPrototypeOf(i.r)===Function.prototype,i.r instanceof Function,Object.isExtensible(i.r),String(i.r).length>0].join()})`);
}
for (const [x, y] of [["a.f", "b.f"], ["a.f", "a.g"], ["a.g", "b.g"], ["fi.r", "a.f"], ["g.f", "a.f"]]) {
  add(`T(()=>{var i=inst(FI0,{env:{f:${x}}}),k=inst(FI0,{env:{f:${y}}});return [i.r===k.r,i.rf()===k.rf(),i.r===${x},k.rf()===${y}].join()})`);
}
const JSF = ["function(){return 7}", "function(x){return x}", "()=>{throw new Error('boom')}", "function(){return {}}", "function(){return 1.5}", "function(){return 2**31}", "function(){return '12'}", "function(){return null}", "function(){return undefined}", "function(){}"];
for (const f of JSF) {
  add(`T(()=>{var j=${f};var i=inst(FI0,{env:{f:j}});return [i.r===j,i.rf()===i.r,D(i.r),i.call()].join()})`);
  add(`T(()=>{var j=${f};var i=inst(FI0,{env:{f:j}});a.t.set(0,i.r);return c.c0(0)})`);
  add(`T(()=>{var j=${f};var i=inst(FI0,{env:{f:j}});a.t.set(0,j);return 0})`);
}

// 7. tabelas e globais importadas: erros de link.
for (const min of [0, 1, 4, 5, 7]) {
  for (const max of [null, 3, 4, 5, 6, 7]) {
    add(`T(()=>{inst(${lit(mkCaller(min, max))},{env:{t:a.t}});return "ok"})`);
    add(`T(()=>{a.t.grow(1);inst(${lit(mkCaller(min, max))},{env:{t:a.t}});return "ok"})`);
  }
}
for (const t of ["a.t", "e.t", "{}", "null", "undefined", "5", "a.f", "WebAssembly.Table.prototype", "new WebAssembly.Table({element:'anyfunc',initial:4,maximum:6})", "new WebAssembly.Table({element:'externref',initial:4,maximum:6})", "b.t", "c.t"]) {
  add(`T(()=>{var i=inst(CL,{env:{t:${t}}});return [i.t===${t},i.c5()].join()})`);
  add(`T(()=>{inst(CL,{env:{t:${t}}});return "ok"})`);
}
add("T(()=>inst(CL))"); add("T(()=>inst(CL,{}))"); add("T(()=>inst(CL,{env:1}))"); add("T(()=>inst(CL,{env:{}}))"); add("T(()=>inst(CL,{env:null}))");
const GPROV = ["5", "5n", "1.5", "NaN", "null", "undefined", "'x'", "a.f", "g.gi", "g.gm", "g.ge", "g.gx", "{}", "j", "a.rf()",
  "new WebAssembly.Global({value:'anyfunc',mutable:false},a.f)", "new WebAssembly.Global({value:'anyfunc',mutable:true},a.f)",
  "new WebAssembly.Global({value:'anyfunc',mutable:true})", "new WebAssembly.Global({value:'externref',mutable:false},{})", "new WebAssembly.Global({value:'externref',mutable:true},{})",
  "new WebAssembly.Global({value:'externref',mutable:true})", "new WebAssembly.Global({value:'i32',mutable:false},1)", "new WebAssembly.Global({value:'i32',mutable:true},1)", "new WebAssembly.Global({value:'i64',mutable:false},1n)"];
for (const [ty, mu] of [[FR, 0], [FR, 1], [ER, 0], [ER, 1], [I32, 0], [I32, 1]]) {
  for (const p of GPROV) {
    add(`T(()=>{var i=inst(${lit(mkGimp(ty, mu))},{env:{g:${p}}});return [D(i.get()),i.g===${p},${mu ? "(i.set(a.f),D(i.get()))" : "0"}].join()})`);
  }
}

// ---- dedupe, filhos e saída.
const known = new Set(knownPrograms("wasm_funcref_bun.tsv", (name) => /^wasm_/.test(name) && name !== "wasm_funcref_bun.tsv"));
const unique = [...new Set(exprs)];
const jobs = [];
let dup = 0;
for (const expr of unique) {
  if (usesHostApi(expr)) continue;
  const source = PRELUDE + `globalThis.R=T(()=>{var {a,b,c,g,e,j,fi}=W();return ${expr}});`;
  if (known.has(source)) { dup++; continue; }
  jobs.push({ expr, source });
}

const runChild = (source) => new Promise((resolve) => {
  const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], timeout: 8000, killSignal: "SIGKILL" });
  const out = [];
  child.stdout.on("data", (chunk) => out.push(chunk));
  child.stderr.on("data", () => {});
  child.on("close", (code) => resolve(code === 0 ? decodeResult(Buffer.concat(out).toString("utf8")) : null));
  child.on("error", () => resolve(null));
  child.stdin.on("error", () => {});
  child.stdin.end(source);
});

(async () => {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    for (;;) {
      const index = next++;
      if (index >= jobs.length) return;
      results[index] = await runChild(jobs[index].source);
    }
  };
  await Promise.all(Array.from({ length: 6 }, worker));
  const dash = new RegExp("[" + String.fromCharCode(0x2013) + String.fromCharCode(0x2014) + "]");
  const rows = [];
  let dropped = 0;
  jobs.forEach((job, index) => {
    const result = results[index];
    if (result === null || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || dash.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      return;
    }
    rows.push({ source: job.source, result });
  });
  process.stderr.write(`mantidos ${rows.length}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
  process.stdout.write(emitFactored("wasm_funcref", rows));
})();
