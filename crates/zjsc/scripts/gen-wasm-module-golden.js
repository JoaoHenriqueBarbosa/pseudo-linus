// Gera tests/golden/wasm_module_bun.tsv: módulos WebAssembly binários montados por helpers JS no prelúdio, medidos no bun.
// Cobre WebAssembly.Module.exports/imports/customSections (ordem e kinds), CompileError com a mensagem exata do bun para
// módulos inválidos (cabeçalho, seção fora de ordem, LEB128, índices, limites, exports duplicados, start, corpo de função,
// tipos de bloco, inicializadores de global), LinkError de WebAssembly.Instance com imports de tipos errados, exports de
// Global/Table/Memory/Tag, Global (mut, i64 BigInt, v128) e Table (externref/anyfunc get/set/grow).
// Cada programa roda num bun filho novo, sem APIs de host, e grava o texto em `globalThis.R`. Programas cuja fonte já
// aparece nos goldens wasm_*_bun.tsv são descartados.
// Colunas: a fonte do programa (JSON) e o valor de `R` (JSON). Uso:
//   bun scripts/gen-wasm-module-golden.js > tests/golden/wasm_module_bun.tsv
const { emitFactored, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const rows = [];
const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

// O filho só avalia o programa; quem imprime `R` é o preload (ver `writeResultPreload` em golden-prelude.js).
if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

const PRELUDE =
  'var u=n=>{var r=[];do{var b=n&127;n>>>=7;if(n)b|=128;r.push(b)}while(n);return r};' +
  'var st=x=>[...u(x.length),...[...x].map(c=>c.charCodeAt(0))];' +
  'var vec=a=>[...u(a.length),...a.flat()];' +
  'var sec=(id,b)=>[id,...u(b.length),...b];' +
  'var M=(...x)=>[0,97,115,109,1,0,0,0,...x.flat()];' +
  'var ft=(p,r)=>[0x60,...u(p.length),...p,...u(r.length),...r];' +
  'var TY=(...f)=>sec(1,vec(f));var FS=(...i)=>sec(3,vec(i.map(x=>[x])));' +
  'var Fn=(code,loc)=>{loc=loc||[];var b=[...u(loc.length),...loc.flat(),...code,0x0b];return[...u(b.length),...b]};' +
  'var CS=(...f)=>sec(10,vec(f));' +
  'var FM=(p,r,code,loc)=>M(TY(ft(p,r)),FS(0),CS(Fn(code,loc)));' +
  'var im=(m,n,...d)=>[...st(m),...st(n),...d.flat()];var IS=(...i)=>sec(2,vec(i));' +
  'var ex=(n,k,i)=>[...st(n),k,...u(i)];var ES=(...e)=>sec(7,vec(e));' +
  'var cu=(n,b)=>sec(0,[...st(n),...b]);' +
  'var T=f=>{try{var v=f();return typeof v==="string"?v:J(v)}catch(e){return e.name+": "+e.message}};' +
  'var J=v=>JSON.stringify(v,(k,x)=>typeof x==="bigint"?x+"n":x===undefined?"undef":x);' +
  'var C=b=>T(()=>{new WebAssembly.Module(new Uint8Array(b));return "ok"});' +
  'var E=b=>T(()=>{var m=new WebAssembly.Module(new Uint8Array(b));return[WebAssembly.Module.exports(m),WebAssembly.Module.imports(m)]});' +
  'var I=(b,o)=>T(()=>{new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(b)),o);return "ok"});' +
  'var P=(v)=>typeof v==="bigint"?v+"n":Object.is(v,-0)?"-0":String(v);\n';

const I32 = 0x7f, I64 = 0x7e, F32 = 0x7d, F64 = 0x7c, V128 = 0x7b, FUNCREF = 0x70, EXTERNREF = 0x6f;
const exprs = [];
const add = (...list) => exprs.push(...list);
const arr = (a) => "[" + a.join(",") + "]";

// ---- 1. cabeçalho e estrutura.
const header = [0, 97, 115, 109, 1, 0, 0, 0];
add("C([])", "C([0])", "C([0,97,115])", "C([0,97,115,109])", "C([0,97,115,109,1])", "C([0,97,115,109,1,0,0])", "C([0,97,115,109,1,0,0,0])");
for (const magic of [[0, 97, 115, 110], [1, 97, 115, 109], [0, 97, 115, 0], [0xff, 0xff, 0xff, 0xff], [0x6d, 0x73, 0x61, 0], [0, 0x41, 0x53, 0x4d]]) {
  add(`C(${arr([...magic, 1, 0, 0, 0])})`);
}
for (const version of [[0, 0, 0, 0], [2, 0, 0, 0], [0, 1, 0, 0], [1, 0, 0, 1], [1, 1, 0, 0], [0x0d, 0, 0, 0], [0xff, 0, 0, 0], [1, 0, 1, 0]]) {
  add(`C(${arr([0, 97, 115, 109, ...version])})`);
}
add("C(M(sec(0,[])))", "C(M(sec(0,[0])))", "C(M(cu('a',[1,2,3])))", "C(M(cu('',[])))", "C(M(sec(0,[5,97])))", "C(M(sec(0,[1,0xff])))");
// Seção fora de ordem e duplicada: todos os pares de ids com conteúdo vazio (vec de zero itens).
const ids = [1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13];
for (const a of ids) for (const b of ids) add(`C(M(sec(${a},[0]),sec(${b},[0])))`);
for (const id of [14, 15, 16, 42, 100, 127, 128, 255]) add(`C(M(sec(${id},[])))`, `C(M(sec(${id},[0])))`);
add("C(M(sec(8,[0])))", "C(M(TY(),sec(8,[0])))", "C(M(TY(ft([],[])),FS(0),sec(8,[0]),CS(Fn([]))))");
// Tamanhos de seção errados.
add("C(M([1,5,0]))", "C(M([1,0]))", "C(M([1,1,0]))", "C(M([1,2,0]))", "C(M([1,0x80,0x00]))", "C(M([1,0x81,0x80,0x80,0x80,0x00,0]))",
  "C(M([1,0x80,0x80,0x80,0x80,0x80,0x00]))", "C(M([1,0xff,0xff,0xff,0xff,0x0f]))", "C(M([1,0xff,0xff,0xff,0xff,0x7f]))",
  "C(M([1]))", "C(M([1,0x80]))", "C(M([0]))", "C(M([0,1]))", "C(M([0,1,0]))", "C(M([0,2,1,0x61]))");
// LEB128 mal formado em contagens e índices.
for (const leb of [[0x80, 0x00], [0x80, 0x80, 0x80, 0x80, 0x00], [0x80, 0x80, 0x80, 0x80, 0x10], [0xff, 0xff, 0xff, 0xff, 0x0f], [0xff, 0xff, 0xff, 0xff, 0x7f],
  [0x80, 0x80, 0x80, 0x80, 0x80, 0x00], [0x80], [0xff], [0x81, 0x00], [0x00]]) {
  add(`C(M(sec(1,[${leb}])))`, `C(M(sec(3,[${leb}])))`, `C(M(TY(ft([],[])),sec(3,[1,${leb}])))`, `C(M(sec(5,[${leb}])))`);
}

// ---- 2. tipos.
for (const form of [0x00, 0x5f, 0x5e, 0x61, 0x7f, 0x40, 0xff]) add(`C(M(sec(1,[1,${form},0,0])))`);
for (const t of [0x00, 0x40, 0x7a, 0x6e, 0x6d, 0x6c, 0x71, 0x72, 0x73, 0x70, 0x6f, 0x7f, 0x7e, 0x7d, 0x7c, 0x7b, 0xff]) {
  add(`C(M(TY(ft([${t}],[]))))`, `C(M(TY(ft([],[${t}]))))`, `E(M(TY(ft([${t}],[${t}])),FS(0),CS(Fn([0x00]))))`);
}
add("C(M(TY(ft([],[0x7f,0x7e]))))", "C(M(TY(ft([0x7f,0x7e,0x7d,0x7c],[0x7f,0x7e,0x7d,0x7c]))))", "C(M(sec(1,[1,0x60,1])))", "C(M(sec(1,[1,0x60])))",
  "C(M(sec(1,[2,0x60,0,0])))", "C(M(sec(1,[0])))", "C(M(sec(1,[0,0])))", "C(M(sec(1,[1,0x60,0,0,0])))", "C(M(sec(1,[1,0x60,0,1])))");
add(`C(M(TY(ft(${arr(Array(1000).fill(I32))},[]))))`, `C(M(TY(ft(${arr(Array(1001).fill(I32))},[]))))`, `C(M(TY(ft([],${arr(Array(1000).fill(I32))}))))`,
  `C(M(TY(ft([],${arr(Array(1001).fill(I32))}))))`);

// ---- 3. função, tabela, memória, tag.
for (const idx of [0, 1, 2, 5, 127, 128, 16383, 16384]) {
  add(`C(M(TY(ft([],[])),FS(${idx}),CS(Fn([]))))`, `C(M(TY(ft([],[]),ft([],[0x7f])),FS(${idx}),CS(Fn([]))))`);
}
add("C(M(FS(0)))", "C(M(TY(ft([],[])),FS(0)))", "C(M(TY(ft([],[])),CS(Fn([]))))", "C(M(TY(ft([],[])),FS(0,0),CS(Fn([]))))",
  "C(M(TY(ft([],[])),FS(0),CS(Fn([]),Fn([]))))", "C(M(TY(ft([],[])),sec(3,[0]),sec(10,[0])))", "C(M(sec(3,[0]),sec(10,[0])))", "C(M(sec(10,[0])))");
const tableDecls = [];
for (const elem of [FUNCREF, EXTERNREF, 0x7f, 0x40, 0x6e, 0x6c, 0x6d, 0x00]) {
  tableDecls.push(`[${elem},0,0]`, `[${elem},0,1]`, `[${elem},1,1,2]`, `[${elem},1,5,2]`, `[${elem},2,0]`, `[${elem},3,1,2]`, `[${elem},0,0x80,0x80,0x80,0x80,0x10]`);
}
for (const d of tableDecls) add(`C(M(sec(4,[1,...${d}])))`);
add("C(M(sec(4,[2,0x70,0,1,0x70,0,1])))", "C(M(sec(4,[0])))", "C(M(sec(4,[1,0x70,0,0x80,0xad,0xe2,0x04])))", "C(M(sec(4,[1,0x70,0,0xa0,0x8d,0x06])))",
  "C(M(sec(4,[1,0x70,0,0xff,0xff,0xff,0xff,0x0f])))", "C(M(sec(4,[1,0x70,1,0xff,0xff,0xff,0xff,0x0f,0xff,0xff,0xff,0xff,0x0f])))",
  "C(M(sec(4,[1,0x70,1,0x80,0xad,0xe2,0x04,0x80,0xad,0xe2,0x04])))", "C(M(sec(4,[1,0x70,1,0x81,0xad,0xe2,0x04,0x80,0xad,0xe2,0x04])))");
for (const decl of ["[0,0]", "[0,1]", "[1,1,1]", "[1,2,1]", "[0,0x80,0x01]", "[0,0x81,0x80,0x04]", "[0,0x80,0x80,0x04]", "[1,0,0x81,0x80,0x04]", "[1,0,0x80,0x80,0x04]",
  "[1,0x80,0x80,0x04,0x80,0x80,0x04]", "[1,0x81,0x80,0x04,0x80,0x80,0x04]", "[2,1]", "[2,1,1]", "[3,1]", "[3,1,1]", "[3,2,1]", "[4,1]", "[4,0]", "[5,1]", "[6,1]",
  "[7,1]", "[0x80,0x01,1]", "[0xff,1]", "[0,0xff,0xff,0xff,0xff,0x0f]", "[1,0xff,0xff,0xff,0xff,0x0f,0xff,0xff,0xff,0xff,0x0f]", "[1,5,3]", "[1,3,3]", "[1,3,4]"]) {
  add(`C(M(sec(5,[1,...${decl}])))`);
}
add("C(M(sec(5,[2,0,1,0,1])))", "C(M(sec(5,[0])))", "C(M(sec(5,[1])))", "C(M(sec(5,[1,0])))", "C(M(sec(5,[1,0,1]),sec(5,[1,0,1])))");
// Tags.
for (const t of [0, 1, 2]) add(`C(M(TY(ft([],[]),ft([0x7f],[])),sec(13,[1,0,${t}])))`, `E(M(TY(ft([],[]),ft([0x7f],[])),sec(13,[1,0,${t}]),ES(ex('t',4,0))))`);
add("C(M(TY(ft([],[0x7f])),sec(13,[1,0,0])))", "C(M(TY(ft([0x7f],[])),sec(13,[1,0,0])))", "C(M(TY(ft([],[])),sec(13,[1,1,0])))", "C(M(TY(ft([],[])),sec(13,[2,0,0,0,0])))");

// ---- 4. global e inicializadores constantes.
const constInit = {
  i32: [0x41, 5, 0x0b], i64: [0x42, 5, 0x0b], f32: [0x43, 0, 0, 0, 0, 0x0b], f64: [0x44, 0, 0, 0, 0, 0, 0, 0, 0, 0x0b],
};
const gtypes = { i32: I32, i64: I64, f32: F32, f64: F64, v128: V128, funcref: FUNCREF, externref: EXTERNREF };
for (const [tn, t] of Object.entries(gtypes)) for (const mut of [0, 1, 2]) {
  for (const [initName, init] of Object.entries(constInit)) {
    add(`C(M(sec(6,[1,${t},${mut},...${arr(init)}])))`);
  }
  add(`C(M(sec(6,[1,${t},${mut},0xd0,${t === FUNCREF ? 0x70 : 0x6f},0x0b])))`);
}
const badInits = [
  [0x41, 1, 0x41, 2, 0x6a, 0x0b], [0x20, 0, 0x0b], [0x23, 0, 0x0b], [0x23, 1, 0x0b], [0x41, 1], [0x0b], [0x41, 1, 0x0b, 0x0b], [0x41, 1, 0x1a, 0x0b], [0x01, 0x41, 1, 0x0b],
  [0x10, 0, 0x0b], [0x41, 1, 0x41, 2, 0x0b], [0x41, 1, 0x00, 0x0b], [0x41, 1, 0x0f, 0x0b], [0x41, 0x80, 0x80, 0x80, 0x80, 0x10, 0x0b], [0x41, 0xff, 0xff, 0xff, 0xff, 0x7f, 0x0b],
  [0x42, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0x0b], [0x43, 0, 0, 0x0b], [0x41, 1, 0x6a, 0x0b], [0x41, 1, 0x6b, 0x0b], [0x41, 1, 0x6c, 0x0b],
  [0x42, 1, 0x42, 2, 0x7c, 0x0b], [0x42, 1, 0x42, 2, 0x7d, 0x0b], [0x42, 1, 0x42, 2, 0x7e, 0x0b], [0x41, 1, 0xa7, 0x0b], [0xd2, 0, 0x0b], [0xd0, 0x70, 0x0b], [0xd0, 0x6f, 0x0b],
  [0xd0, 0x7f, 0x0b], [0xd1, 0x0b], [0x04, 0x40, 0x0b, 0x0b], [0x02, 0x40, 0x0b, 0x0b], [0x41, 1, 0x41, 2, 0x41, 3, 0x6a, 0x6a, 0x0b], [0xfd, 0x0c, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0b],
];
for (const init of badInits) {
  for (const t of [I32, I64, FUNCREF, EXTERNREF]) add(`C(M(TY(ft([],[])),FS(0),sec(6,[1,${t},0,...${arr(init)}]),CS(Fn([]))))`);
}
add("C(M(sec(6,[1,0x7f,0])))", "C(M(sec(6,[1,0x7f])))", "C(M(sec(6,[1])))", "C(M(sec(6,[0])))", "C(M(sec(6,[2,0x7f,0,0x41,1,0x0b,0x7f,0,0x23,0,0x0b])))",
  "C(M(sec(6,[2,0x7f,1,0x41,1,0x0b,0x7f,0,0x23,0,0x0b])))", "C(M(sec(6,[2,0x7f,0,0x41,1,0x0b,0x7e,0,0x23,0,0x0b])))",
  "C(M(IS(im('m','g',[3,0x7f,0])),sec(6,[1,0x7f,0,0x23,0,0x0b])))", "C(M(IS(im('m','g',[3,0x7f,1])),sec(6,[1,0x7f,0,0x23,0,0x0b])))",
  "C(M(IS(im('m','g',[3,0x7e,0])),sec(6,[1,0x7f,0,0x23,0,0x0b])))", "C(M(IS(im('m','g',[3,0x7f,0])),sec(6,[1,0x7f,0,0x23,1,0x0b])))");

// ---- 5. export.
const exportCases = [
  ["ES(ex('a',0,0))", ""], ["ES(ex('a',0,1))", ""], ["ES(ex('a',1,0))", ""], ["ES(ex('a',2,0))", ""], ["ES(ex('a',3,0))", ""], ["ES(ex('a',4,0))", ""],
  ["ES(ex('a',5,0))", ""], ["ES(ex('a',0x7f,0))", ""], ["ES(ex('a',0,0),ex('a',0,0))", ""], ["ES(ex('a',0,0),ex('b',0,0))", ""], ["ES(ex('a',0,0),ex('a',1,0))", ""],
  ["ES(ex('',0,0))", ""], ["ES(ex('a b',0,0),ex('A B',0,0))", ""], ["ES(ex('\\u00e9',0,0))", ""],
];
const exportBases = {
  func: "TY(ft([],[])),FS(0),", table: "TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),", mem: "sec(5,[1,0,1]),", glob: "sec(6,[1,0x7f,0,0x41,1,0x0b]),",
  tag: "TY(ft([],[])),sec(13,[1,0,0]),", none: "",
};
for (const [e] of exportCases) {
  for (const [bn, base] of Object.entries(exportBases)) {
    // a ordem das seções importa: tipo, função, tabela, memória, tag, global, export, código.
    const code = bn === "func" || bn === "table" || bn === "tag" ? ",CS(Fn([]))" : "";
    const codeFix = bn === "tag" ? "" : code;
    add(`C(M(${base}${e}${codeFix}))`);
  }
}
add("C(M(sec(7,[1,1,0x61,0,0])))", "C(M(sec(7,[1,0,0,0])))", "C(M(sec(7,[1,2,0x61,0])))", "C(M(sec(7,[1,1,0xff,0,0])))", "C(M(sec(7,[1,2,0xc3,0x28,0,0])))",
  "C(M(sec(7,[1,2,0xc0,0xaf,0,0])))", "C(M(sec(7,[1,3,0xed,0xa0,0x80,0,0])))", "C(M(sec(7,[1,4,0xf4,0x90,0x80,0x80,0,0])))", "C(M(sec(7,[1,4,0xf0,0x9f,0x98,0x80,0,0])))",
  "C(M(sec(7,[1,5,0x61,0x62,0x63,0,0])))", "C(M(sec(7,[2,1,0x61,0,0])))", "C(M(sec(7,[0])))", "C(M(sec(7,[1])))", "C(M(sec(7,[1,0x80,0x01,0x61])))");

// ---- 6. start.
for (const [p, r, idx] of [[[], [], 0], [[I32], [], 0], [[], [I32], 0], [[I32], [I32], 0], [[], [], 1], [[], [], 7], [[F64, I64], [], 0], [[], [I32, I32], 0]]) {
  add(`C(M(TY(ft(${arr(p)},${arr(r)})),FS(0),sec(8,[${idx}]),CS(Fn(${arr(r.length ? [0x41, 0] : [])}))))`);
}
add("C(M(sec(8,[0])))", "C(M(sec(8,[])))", "C(M(sec(8,[0x80,0x00])))", "C(M(TY(ft([],[])),FS(0),sec(8,[0]),sec(8,[0]),CS(Fn([]))))",
  "C(M(IS(im('m','f',[0,0])),TY(ft([],[])),sec(8,[0])))", "C(M(TY(ft([],[])),IS(im('m','f',[0,0])),sec(8,[0])))");

// ---- 7. element e data.
const elemFlags = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
for (const f of elemFlags) {
  add(`C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,${f},0x41,0,0x0b,0,1,0]),CS(Fn([]))))`);
  add(`C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,${f},0x00,1,0]),CS(Fn([]))))`);
  add(`C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,${f},0x41,0,0x0b,0x70,1,0xd2,0,0x0b]),CS(Fn([]))))`);
}
for (const fi of [0, 1, 2, 100]) add(`C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,0,0x41,0,0x0b,1,${fi}]),CS(Fn([]))))`);
for (const off of [[0x41, 0, 0x0b], [0x42, 0, 0x0b], [0x20, 0, 0x0b], [0x23, 0, 0x0b], [0x41, 0], [0x0b], [0xd0, 0x70, 0x0b], [0x41, 1, 0x41, 1, 0x6a, 0x0b]]) {
  add(`C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,0,...${arr(off)},1,0]),CS(Fn([]))))`);
  add(`C(M(sec(5,[1,0,1]),sec(11,[1,0,...${arr(off)},1,0x61])))`);
}
add("C(M(sec(9,[1,0,0x41,0,0x0b,0])))", "C(M(sec(9,[1,0,0x41,0,0x0b,1,0])))", "C(M(TY(ft([],[])),FS(0),sec(9,[1,0,0x41,0,0x0b,1,0]),CS(Fn([]))))",
  "C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,2,1,0x41,0,0x0b,0,1,0]),CS(Fn([]))))", "C(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(9,[1,2,5,0x41,0,0x0b,0,1,0]),CS(Fn([]))))");
for (const f of [0, 1, 2, 3, 4]) {
  add(`C(M(sec(5,[1,0,1]),sec(11,[1,${f},0x41,0,0x0b,1,0x61])))`);
  add(`C(M(sec(5,[1,0,1]),sec(11,[1,${f},0,0x41,0,0x0b,1,0x61])))`);
  add(`C(M(sec(5,[1,0,1]),sec(11,[1,${f},1,1,0x61])))`);
  add(`C(M(sec(11,[1,${f},0x41,0,0x0b,1,0x61])))`);
}
add("C(M(sec(5,[1,0,1]),sec(11,[1,0,0x41,0,0x0b,5,0x61])))", "C(M(sec(5,[1,0,1]),sec(11,[2,0,0x41,0,0x0b,1,0x61])))", "C(M(sec(5,[1,0,1]),sec(11,[1,0,0x41,0,0x0b,0])))",
  "C(M(sec(5,[1,0,1]),sec(12,[1]),sec(11,[1,0,0x41,0,0x0b,1,0x61])))", "C(M(sec(5,[1,0,1]),sec(12,[2]),sec(11,[1,0,0x41,0,0x0b,1,0x61])))",
  "C(M(sec(5,[1,0,1]),sec(12,[0]),sec(11,[1,0,0x41,0,0x0b,1,0x61])))", "C(M(sec(5,[1,0,1]),sec(12,[1]),sec(11,[0])))", "C(M(sec(12,[0]),sec(11,[0])))",
  "C(M(TY(ft([],[])),FS(0),sec(12,[0]),CS(Fn([0x00]))))");
add("C(M(TY(ft([],[])),FS(0),CS(Fn([0xfc,0x08,0,0,0x0b]))))", "C(M(TY(ft([],[])),FS(0),sec(5,[1,0,1]),sec(12,[1]),CS(Fn([0x41,0,0x41,0,0x41,0,0xfc,0x08,0,0,0x0b]))))",
  "C(M(TY(ft([],[])),FS(0),sec(5,[1,0,1]),CS(Fn([0x41,0,0x41,0,0x41,0,0xfc,0x08,0,0,0x0b]))))");

// ---- 8. corpo de função: stack mismatch, br, tipos de bloco.
const bodies = [
  [[], [], []], [[], [], [0x1a]], [[], [], [0x41, 0]], [[], [I32], []], [[], [I32], [0x41, 0]], [[], [I32], [0x42, 0]], [[], [I32], [0x41, 0, 0x41, 0]], [[], [I32], [0x00]],
  [[], [I32], [0x0f]], [[], [I32], [0x41, 0, 0x0f]], [[], [], [0x41, 0, 0x0f]], [[I32], [I32], [0x20, 0]], [[I32], [I32], [0x20, 1]], [[I32], [I64], [0x20, 0]],
  [[I32, I32], [I32], [0x20, 0, 0x20, 1, 0x6a]], [[I32], [I32], [0x20, 0, 0x20, 0, 0x6a, 0x6a]], [[], [I32], [0x6a]], [[], [], [0x6a]], [[], [], [0x01]], [[], [], [0x00]],
  [[], [I32], [0x00, 0x41, 0]], [[], [I32], [0x00, 0x42, 0]], [[], [], [0x0c, 0]], [[], [], [0x0c, 1]], [[], [], [0x0c, 5]], [[], [], [0x0d, 0]], [[], [], [0x41, 0, 0x0d, 0]],
  [[], [], [0x41, 0, 0x0d, 1]], [[], [], [0x41, 0, 0x0e, 0, 0]], [[], [], [0x41, 0, 0x0e, 1, 0, 0]], [[], [], [0x41, 0, 0x0e, 1, 5, 0]], [[], [], [0x41, 0, 0x0e, 0, 5]],
  [[], [], [0x02, 0x40, 0x0c, 0, 0x0b]], [[], [], [0x02, 0x40, 0x0c, 1, 0x0b]], [[], [], [0x02, 0x40, 0x0c, 2, 0x0b]], [[], [], [0x03, 0x40, 0x0c, 0, 0x0b]],
  [[], [], [0x03, 0x40, 0x0c, 1, 0x0b]], [[], [I32], [0x02, 0x7f, 0x41, 0, 0x0c, 0, 0x0b]], [[], [I32], [0x02, 0x7f, 0x0c, 0, 0x0b]], [[], [I32], [0x02, 0x7f, 0x0b]],
  [[], [I32], [0x02, 0x7f, 0x41, 0, 0x0b]], [[], [], [0x02, 0x7f, 0x41, 0, 0x0b]], [[], [], [0x02, 0x7f, 0x41, 0, 0x0b, 0x1a]], [[], [I32], [0x03, 0x7f, 0x41, 0, 0x0b]],
  [[], [], [0x04, 0x40, 0x0b]], [[], [], [0x41, 0, 0x04, 0x40, 0x0b]], [[], [], [0x41, 0, 0x04, 0x40, 0x05, 0x0b]], [[], [], [0x41, 0, 0x04, 0x40, 0x05, 0x05, 0x0b]],
  [[], [I32], [0x41, 0, 0x04, 0x7f, 0x41, 1, 0x0b]], [[], [I32], [0x41, 0, 0x04, 0x7f, 0x41, 1, 0x05, 0x41, 2, 0x0b]], [[], [I32], [0x41, 0, 0x04, 0x7f, 0x41, 1, 0x05, 0x42, 2, 0x0b]],
  [[], [], [0x42, 0, 0x04, 0x40, 0x0b]], [[], [], [0x05]], [[], [], [0x0b, 0x0b]], [[], [], [0x02, 0x40]], [[], [], [0x02, 0x40, 0x0b, 0x0b]],
  [[], [], [0x02, 0x41, 0x0b]], [[], [], [0x02, 0x00, 0x0b]], [[], [], [0x02, 0x01, 0x0b]], [[], [], [0x02, 0x7f, 0x0b]], [[], [], [0x02, 0x6f, 0x0b]], [[], [], [0x02, 0x70, 0x0b]],
  [[], [], [0x02, 0x7b, 0x0b]], [[], [], [0x02, 0x7a, 0x0b]], [[], [], [0x02, 0x80, 0x0b]], [[], [], [0x02, 0xc0, 0x00, 0x0b]], [[], [], [0x03, 0x01, 0x0b]], [[], [], [0x04, 0x05, 0x0b]],
  [[], [], [0x10, 0]], [[], [], [0x10, 1]], [[], [], [0x10, 0x80, 0x00]], [[], [], [0x11, 0, 0]], [[], [], [0x41, 0, 0x11, 0, 0]], [[], [], [0x41, 0, 0x11, 0, 5]],
  [[], [], [0x20, 0]], [[], [], [0x21, 0]], [[], [], [0x22, 0]], [[I32], [], [0x41, 0, 0x21, 0]], [[I32], [], [0x42, 0, 0x21, 0]], [[I32], [I32], [0x41, 0, 0x22, 0]],
  [[], [], [0x23, 0]], [[], [], [0x24, 0]], [[], [], [0x41, 0, 0x24, 0]], [[], [], [0x28, 2, 0, 0x1a]], [[], [], [0x41, 0, 0x28, 2, 0, 0x1a]], [[], [], [0x41, 0, 0x28, 2, 0, 0x1a]],
  [[], [], [0x3f, 0, 0x1a]], [[], [], [0x41, 0, 0x40, 0, 0x1a]], [[], [], [0xff]], [[], [], [0xfe, 0x00]], [[], [], [0xfc, 0xff, 0x00]], [[], [], [0xfd, 0xff, 0xff, 0x03]],
  [[], [], [0x41, 0, 0x1b]], [[], [], [0x41, 0, 0x41, 0, 0x41, 0, 0x1b, 0x1a]], [[], [], [0x41, 0, 0x42, 0, 0x41, 0, 0x1b, 0x1a]], [[], [], [0x41, 0, 0x41, 0, 0x41, 0, 0x1c, 0, 0x1a]],
  [[], [], [0x41, 0, 0x1c, 1, 0x7f]], [[], [], [0x41, 0, 0x41, 0, 0x41, 0, 0x1c, 1, 0x7f, 0x1a]], [[], [], [0xd0, 0x70, 0x1a]], [[], [], [0xd0, 0x6f, 0x1a]], [[], [], [0xd0, 0x7f, 0x1a]],
  [[], [], [0xd2, 0, 0x1a]], [[], [], [0xd1, 0x1a]], [[], [], [0x41, 0, 0xd1, 0x1a]], [[], [], [0x41, 0, 0xd4, 0x1a]], [[], [], [0x44, 0, 0, 0, 0, 0, 0, 0, 0, 0x1a]],
  [[], [], [0x44, 0, 0, 0, 0, 0, 0, 0, 0x1a]], [[], [], [0x43, 0, 0, 0, 0x1a]], [[], [], [0x42, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01, 0x1a]],
  [[], [], [0x41, 0x80, 0x80, 0x80, 0x80, 0x08, 0x1a]], [[], [], [0x41, 0x80, 0x80, 0x80, 0x80, 0x78, 0x1a]], [[], [], [0x41, 0xff, 0xff, 0xff, 0xff, 0x0f, 0x1a]],
  [[], [I32], [0x41, 0, 0x45]], [[], [I32], [0x42, 0, 0x45]], [[], [I32], [0x42, 0, 0xa7]], [[], [I64], [0x41, 0, 0xac]], [[], [I64], [0x41, 0, 0xad]], [[], [F32], [0x41, 0, 0xb2]],
  [[], [F32], [0x41, 0, 0xbc]], [[], [I32], [0x43, 0, 0, 0, 0, 0xbc]], [[], [I64], [0x41, 0, 0xbc]], [[], [I32], [0x41, 0, 0x41, 0, 0x46]], [[], [I32], [0x41, 0, 0x42, 0, 0x46]],
  [[], [I32], [0x41, 0, 0x41, 0, 0x4a, 0x41, 0, 0x4b]], [[], [I32], [0x42, 0, 0x42, 0, 0x51]], [[], [I32], [0x43, 0, 0, 0, 0, 0x43, 0, 0, 0, 0, 0x5b]], [[], [I32], [0x44, 0, 0, 0, 0, 0, 0, 0, 0, 0x44, 0, 0, 0, 0, 0, 0, 0, 0, 0x61]],
];
for (const [p, r, code] of bodies) add(`C(FM(${arr(p)},${arr(r)},${arr(code)}))`);
// Locals.
for (const loc of ["[[1,0x7f]]", "[[0,0x7f]]", "[[1,0x40]]", "[[1,0x00]]", "[[0xffffffff,0x7f]]", "[[0x10000000,0x7f]]", "[[50000,0x7f]]", "[[50001,0x7f]]", "[[1,0x7f],[1,0x7e]]",
  "[[1,0x7b]]", "[[1,0x70]]", "[[1,0x6f]]", "[[1,0x6e]]"]) {
  add(`C(FM([],[],[],${loc}))`, `C(FM([0x7f],[],[0x20,1,0x1a],${loc}))`);
}
add("C(M(TY(ft([],[])),FS(0),sec(10,[1,0])))", "C(M(TY(ft([],[])),FS(0),sec(10,[1,1,0])))", "C(M(TY(ft([],[])),FS(0),sec(10,[1,2,0,0x0b])))", "C(M(TY(ft([],[])),FS(0),sec(10,[1,3,0,0x0b])))",
  "C(M(TY(ft([],[])),FS(0),sec(10,[1,2,0,0x0b,0])))", "C(M(TY(ft([],[])),FS(0),sec(10,[1,2,0,0x00])))", "C(M(TY(ft([],[])),FS(0),sec(10,[1,1,0x0b])))",
  "C(M(TY(ft([],[])),FS(0),sec(10,[2,2,0,0x0b])))", "C(M(TY(ft([],[])),FS(0),sec(10,[1,0x80,0x00,0,0x0b])))", "C(M(TY(ft([],[])),FS(0),sec(10,[0])))");

// ---- 9. exports/imports/customSections de módulos válidos.
const kinds = {
  func: "TY(ft([],[])),FS(0),", table: "sec(4,[1,0x70,0,1]),", mem: "sec(5,[1,0,1]),", glob: "sec(6,[1,0x7f,0,0x41,1,0x0b]),", tag: "TY(ft([],[])),sec(13,[1,0,0]),",
};
add("E(M())", "E(M(TY(ft([],[]))))");
const importDescs = {
  func: "[0,0]", table: "[1,0x70,0,1]", tableExt: "[1,0x6f,1,1,5]", mem: "[2,0,1]", memMax: "[2,1,1,3]", memShared: "[2,3,1,3]", globI32: "[3,0x7f,0]", globI32m: "[3,0x7f,1]",
  globI64: "[3,0x7e,0]", globF32: "[3,0x7d,0]", globF64m: "[3,0x7c,1]", globExt: "[3,0x6f,0]", globFn: "[3,0x70,1]", globV128: "[3,0x7b,0]", tag: "[4,0,0]",
};
for (const [n, d] of Object.entries(importDescs)) {
  add(`E(M(TY(ft([],[])),IS(im('m','${n}',${d}))))`, `E(M(TY(ft([],[])),IS(im('',${JSON.stringify(n)},${d}))))`, `E(M(TY(ft([],[])),IS(im('mod\\u00e9','\\ud83d\\ude00${n}',${d}))))`);
}
const names = Object.keys(importDescs);
for (let i = 0; i < names.length; i++) for (let j = 0; j < names.length; j++) {
  if (i === j) continue;
  add(`E(M(TY(ft([],[])),IS(im('a','${names[i]}',${importDescs[names[i]]}),im('b','${names[j]}',${importDescs[names[j]]}))))`);
}
// Exports em todas as ordens e kinds, com índices variados.
const kindOrder = ["func", "table", "mem", "glob", "tag"];
const kindCode = { func: 0, table: 1, mem: 2, glob: 3, tag: 4 };
for (const a of kindOrder) for (const b of kindOrder) {
  const base = [...new Set([a, b])].map(k => kinds[k]);
  const sorted = ["TY(ft([],[])),FS(0),", "sec(4,[1,0x70,0,1]),", "sec(5,[1,0,1]),", "TY(ft([],[])),sec(13,[1,0,0]),", "sec(6,[1,0x7f,0,0x41,1,0x0b]),"];
  void base; void sorted;
  const parts = [];
  const need = new Set([a, b]);
  if (need.has("func") || need.has("tag")) parts.push("TY(ft([],[]))");
  if (need.has("func")) parts.push("FS(0)");
  if (need.has("table")) parts.push("sec(4,[1,0x70,0,1])");
  if (need.has("mem")) parts.push("sec(5,[1,0,1])");
  if (need.has("tag")) parts.push("sec(13,[1,0,0])");
  if (need.has("glob")) parts.push("sec(6,[1,0x7f,0,0x41,1,0x0b])");
  const exportsPart = a === b ? `ES(ex('x',${kindCode[a]},0))` : `ES(ex('z',${kindCode[a]},0),ex('a',${kindCode[b]},0))`;
  parts.push(exportsPart);
  if (need.has("func")) parts.push("CS(Fn([]))");
  add(`E(M(${parts.join(",")}))`);
}
// Seções customizadas.
const csModules = [
  "M()", "M(cu('a',[1,2]))", "M(cu('a',[1,2]),cu('a',[3]))", "M(cu('a',[1]),cu('b',[2]),cu('a',[3]))", "M(cu('',[9]))", "M(cu('\\u00e9',[7,8]))", "M(TY(ft([],[])),cu('a',[1]),FS(0),cu('a',[2]),CS(Fn([])),cu('a',[3]))",
  "M(cu('abc',[]))", "M(cu('a',Array(300).fill(7)))", "M(cu('name',[1,2,3]))", "M(cu('\\ud83d\\ude00',[1]))", "M(cu('a',[255,0,128]))",
];
const csArgs = ["'a'", "'b'", "''", "'name'", "'\\u00e9'", "'\\ud83d\\ude00'", "'abc'", "undefined", "null", "1", "{toString(){return 'a'}}", "Symbol()"];
for (const m of csModules) for (const a of csArgs) {
  add(`T(()=>{var m=new WebAssembly.Module(new Uint8Array(${m}));var r=WebAssembly.Module.customSections(m,${a});return r.map(b=>[b.constructor.name,b.byteLength,[...new Uint8Array(b)].join('.')].join(':')).join('|')+'#'+r.length})`);
}
add("T(()=>WebAssembly.Module.customSections(new WebAssembly.Module(new Uint8Array(M())))).length", "T(()=>WebAssembly.Module.customSections())", "T(()=>WebAssembly.Module.customSections({},'a'))",
  "T(()=>WebAssembly.Module.customSections(1,'a'))", "T(()=>WebAssembly.Module.exports())", "T(()=>WebAssembly.Module.exports({}))", "T(()=>WebAssembly.Module.exports(null))",
  "T(()=>WebAssembly.Module.imports())", "T(()=>WebAssembly.Module.imports(1))", "T(()=>WebAssembly.Module.imports(new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M())))))",
  "T(()=>{var m=new WebAssembly.Module(new Uint8Array(M(cu('a',[1]))));var b=WebAssembly.Module.customSections(m,'a')[0];return [Object.isFrozen(b),b.resizable,b.maxByteLength]})",
  "T(()=>{var m=new WebAssembly.Module(new Uint8Array(M(cu('a',[1]))));return WebAssembly.Module.customSections(m,'a')[0]===WebAssembly.Module.customSections(m,'a')[0]})",
  "T(()=>{var m=new WebAssembly.Module(new Uint8Array(M(TY(ft([],[])),FS(0),ES(ex('f',0,0)),CS(Fn([])))));var e=WebAssembly.Module.exports(m);return [Object.keys(e[0]),Object.isFrozen(e),Object.isFrozen(e[0]),e===WebAssembly.Module.exports(m)]})",
  "T(()=>{var m=new WebAssembly.Module(new Uint8Array(M(IS(im('m','f',[0,0])),TY(ft([],[])))));var e=WebAssembly.Module.imports(m);return [Object.keys(e[0]),Object.getPrototypeOf(e[0])===Object.prototype]})");
// Ordem dos exports (repetida em módulo com muitos itens).
const manyExports = [];
for (let i = 0; i < 8; i++) manyExports.push(`ex('e${(i * 5) % 8}',${[0, 2, 3, 1][i % 4]},0)`);
add(`E(M(TY(ft([],[])),FS(0),sec(4,[1,0x70,0,1]),sec(5,[1,0,1]),sec(6,[1,0x7f,0,0x41,1,0x0b]),ES(${manyExports.join(",")}),CS(Fn([]))))`);
add("E(M(TY(ft([],[])),FS(0),ES(ex('b',0,0),ex('a',0,0),ex('c',0,0)),CS(Fn([]))))", "E(M(TY(ft([],[])),FS(0),ES(ex('10',0,0),ex('9',0,0),ex('2',0,0),ex('-1',0,0)),CS(Fn([]))))");

// ---- 10. Instance com imports errados (LinkError).
const funcVals = ["undefined", "null", "1", "'f'", "{}", "[]", "()=>1", "function(){}", "class A{}", "async()=>1", "function*(){}", "Math.max", "(()=>1).bind(null)", "new Proxy(function(){},{})", "Symbol()", "true", "1n", "WebAssembly.Module", "new WebAssembly.Global({value:'i32'})"];
for (const v of funcVals) add(`I(M(TY(ft([],[])),IS(im('m','f',[0,0]))),{m:{f:${v}}})`, `I(M(TY(ft([0x7f],[0x7f])),IS(im('m','f',[0,0]))),{m:{f:${v}}})`);
const modVals = ["undefined", "null", "1", "'m'", "true", "Symbol()", "1n", "()=>1", "function(){}", "[]", "{}", "{f(){}}", "new Proxy({},{})", "Object.create({f(){}})"];
for (const v of modVals) add(`I(M(TY(ft([],[])),IS(im('m','f',[0,0]))),{m:${v}})`, `I(M(TY(ft([],[])),IS(im('m','f',[0,0]))),${v})`);
add("I(M(TY(ft([],[])),IS(im('m','f',[0,0]))))", "I(M(TY(ft([],[])),IS(im('m','f',[0,0]))),{})", "I(M(TY(ft([],[])),IS(im('m','f',[0,0]))),{m:{}})", "I(M())", "I(M(),undefined)", "I(M(),null)", "I(M(),1)",
  "I(M(),{})", "I(M(TY(ft([],[])),IS(im('m','f',[0,0]))),{m:{f(){}}},1)", "I(M(IS(im('a','x',[2,0,1]),im('b','y',[0,0]))))");
const globVals = ["undefined", "null", "1", "1.5", "-1", "NaN", "Infinity", "2**31", "2**32", "'1'", "''", "true", "{}", "1n", "BigInt(2**40)", "Symbol()", "{valueOf(){return 3}}",
  "new WebAssembly.Global({value:'i32'},1)", "new WebAssembly.Global({value:'i32',mutable:true},1)", "new WebAssembly.Global({value:'i64'},1n)", "new WebAssembly.Global({value:'i64',mutable:true},1n)",
  "new WebAssembly.Global({value:'f32'},1)", "new WebAssembly.Global({value:'f64'},1)", "new WebAssembly.Global({value:'f64',mutable:true},1)", "new WebAssembly.Global({value:'externref'},1)",
  "new WebAssembly.Global({value:'anyfunc'},null)", "new WebAssembly.Global({value:'externref',mutable:true},1)"];
const globTypes = [["i32", "0x7f"], ["i64", "0x7e"], ["f32", "0x7d"], ["f64", "0x7c"], ["externref", "0x6f"], ["funcref", "0x70"]];
for (const [, tc] of globTypes) for (const mut of [0, 1]) for (const v of globVals) add(`I(M(IS(im('m','g',[3,${tc},${mut}]))),{m:{g:${v}}})`);
add("I(M(IS(im('m','g',[3,0x7b,0]))),{m:{g:1}})", "I(M(IS(im('m','g',[3,0x7b,1]))),{m:{g:1}})", "I(M(IS(im('m','g',[3,0x7b,0]))),{m:{g:{}}})");
const memVals = ["undefined", "null", "1", "{}", "()=>1", "new WebAssembly.Table({element:'anyfunc',initial:1})", "new WebAssembly.Memory({initial:1})", "new WebAssembly.Memory({initial:0})",
  "new WebAssembly.Memory({initial:2})", "new WebAssembly.Memory({initial:1,maximum:2})", "new WebAssembly.Memory({initial:1,maximum:3})", "new WebAssembly.Memory({initial:1,maximum:4})",
  "new WebAssembly.Memory({initial:2,maximum:3})", "new WebAssembly.Memory({initial:3,maximum:3})", "new WebAssembly.Memory({initial:1,maximum:3,shared:true})", "new WebAssembly.Memory({initial:3,maximum:3,shared:true})",
  "new WebAssembly.Memory({initial:1,maximum:65536})", "new Proxy(new WebAssembly.Memory({initial:1}),{})", "Object.create(WebAssembly.Memory.prototype)"];
for (const d of ["[2,0,1]", "[2,0,0]", "[2,0,2]", "[2,1,1,3]", "[2,1,0,3]", "[2,1,2,2]", "[2,3,1,3]", "[2,3,3,3]"]) for (const v of memVals) add(`I(M(IS(im('m','x',${d}))),{m:{x:${v}}})`);
const tabVals = ["undefined", "null", "1", "{}", "new WebAssembly.Memory({initial:1})", "new WebAssembly.Table({element:'anyfunc',initial:1})", "new WebAssembly.Table({element:'anyfunc',initial:0})",
  "new WebAssembly.Table({element:'anyfunc',initial:2})", "new WebAssembly.Table({element:'anyfunc',initial:1,maximum:2})", "new WebAssembly.Table({element:'anyfunc',initial:1,maximum:5})",
  "new WebAssembly.Table({element:'anyfunc',initial:1,maximum:6})", "new WebAssembly.Table({element:'externref',initial:1})", "new WebAssembly.Table({element:'externref',initial:1,maximum:5})",
  "new WebAssembly.Table({element:'externref',initial:5,maximum:5})", "new WebAssembly.Table({element:'anyfunc',initial:5,maximum:5})", "new WebAssembly.Table({element:'anyfunc',initial:6,maximum:6})"];
for (const d of ["[1,0x70,0,1]", "[1,0x70,0,0]", "[1,0x70,1,1,5]", "[1,0x70,1,2,5]", "[1,0x6f,0,1]", "[1,0x6f,1,1,5]", "[1,0x6f,1,5,5]"]) for (const v of tabVals) add(`I(M(IS(im('m','x',${d}))),{m:{x:${v}}})`);
const tagVals = ["undefined", "null", "1", "{}", "new WebAssembly.Tag({parameters:[]})", "new WebAssembly.Tag({parameters:['i32']})", "new WebAssembly.Tag({parameters:['i64']})", "new WebAssembly.Tag({parameters:['i32','f32']})",
  "new WebAssembly.Tag({parameters:['externref']})", "()=>1"];
for (const d of ["[4,0,0]", "[4,0,1]"]) for (const v of tagVals) add(`I(M(TY(ft([],[]),ft([0x7f],[])),IS(im('m','x',${d}))),{m:{x:${v}}})`);
// Vários imports: a ordem em que o erro aparece.
add("I(M(TY(ft([],[])),IS(im('a','f',[0,0]),im('b','g',[3,0x7f,0]))),{a:{f:1},b:{g:'x'}})", "I(M(TY(ft([],[])),IS(im('a','f',[0,0]),im('b','g',[3,0x7f,0]))),{a:{f(){}},b:{g:'x'}})",
  "I(M(TY(ft([],[])),IS(im('a','f',[0,0]),im('b','g',[3,0x7f,0]))),{a:{f(){}}})", "I(M(TY(ft([],[])),IS(im('a','f',[0,0]),im('b','g',[3,0x7f,0]))),{b:{g:1}})",
  "I(M(TY(ft([],[])),IS(im('a','f',[0,0]),im('a','f',[0,0]))),{a:{f(){}}})", "I(M(TY(ft([],[])),IS(im('a','f',[0,0]),im('a','f',[3,0x7f,0]))),{a:{f(){}}})");
// Imports de função wasm exportada com assinatura diferente.
for (const [p1, r1, p2, r2] of [[[], [], [], []], [[I32], [], [], []], [[], [I32], [], []], [[I32], [I32], [I32], [I32]], [[I32], [I32], [I64], [I32]], [[I32], [I32], [I32], [I64]], [[I32, I32], [], [I32], []]]) {
  add(`I(M(TY(ft(${arr(p1)},${arr(r1)})),IS(im('m','f',[0,0]))),{m:{f:(new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M(TY(ft(${arr(p2)},${arr(r2)})),FS(0),ES(ex('f',0,0)),CS(Fn(${arr(r2.length ? (r2[0] === I64 ? [0x42, 0] : [0x41, 0]) : [])})))))).exports.f)}})`);
}

// ---- 11. exports de Global/Table/Memory/Tag e valores.
const instOf = (body, imp) => `new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M(${body}))),${imp || "undefined"}).exports`;
for (const [tn, tc, init] of [["i32", I32, "0x41,7"], ["i64", I64, "0x42,7"], ["f32", F32, "0x43,0,0,0xc0,0x3f"], ["f64", F64, "0x44,0,0,0,0,0,0,0xf8,0x3f"], ["externref", EXTERNREF, "0xd0,0x6f"], ["funcref", FUNCREF, "0xd0,0x70"]]) {
  for (const mut of [0, 1]) {
    const g = `sec(6,[1,${tc},${mut},${init},0x0b]),ES(ex('g',3,0))`;
    add(`T(()=>{var g=${instOf(g)}.g;return [Object.prototype.toString.call(g),g.value===undefined?'u':P(g.value),g.valueOf===WebAssembly.Global.prototype.valueOf,Object.getPrototypeOf(g)===WebAssembly.Global.prototype]})`);
    add(`T(()=>{var g=${instOf(g)}.g;g.value=${tn === "i64" ? "5n" : tn.startsWith("f") || tn === "i32" ? "5" : "{}"};return P(g.value)})`);
    add(`T(()=>{var g=${instOf(g)}.g;g.value=${tn === "i64" ? "5" : "5n"};return P(g.value)})`);
    add(`T(()=>{var e=${instOf(g)};return e.g===e.g})`);
    add(`T(()=>{var e=${instOf(g)};return Object.getOwnPropertyDescriptor(e,'g').writable+','+Object.isFrozen(e)+','+Object.getPrototypeOf(e)})`);
  }
}
add(`T(()=>{var e=${instOf("TY(ft([],[])),FS(0),sec(4,[1,0x70,0,2]),sec(5,[1,0,2]),ES(ex('f',0,0),ex('t',1,0),ex('m',2,0)),CS(Fn([]))")};return [typeof e.f,e.f.name,e.f.length,e.t.length,e.m.buffer.byteLength,Object.keys(e).join(),Object.getPrototypeOf(e)===null,Object.isFrozen(e)]})`);
add(`T(()=>{var e=${instOf("sec(4,[1,0x70,0,2]),ES(ex('t',1,0))")};return [e.t.length,e.t.get(0),e.t.get(1),e.t.grow(1),e.t.length,e.t.get(2)]})`);
add(`T(()=>{var e=${instOf("sec(4,[1,0x70,1,1,2]),ES(ex('t',1,0))")};e.t.grow(1);return e.t.grow(1)})`);
add(`T(()=>{var e=${instOf("sec(4,[1,0x6f,1,1,2]),ES(ex('t',1,0))")};return [e.t.get(0),e.t.grow(1,'x'),e.t.get(1),e.t.length]})`);
add(`T(()=>{var e=${instOf("sec(4,[1,0x6f,0,2]),ES(ex('t',1,0))")};e.t.set(0,{a:1});e.t.set(1,undefined);return [typeof e.t.get(0),e.t.get(1)]})`);
add(`T(()=>{var e=${instOf("sec(5,[1,0,2]),ES(ex('m',2,0))")};var b=e.m.buffer;var r=e.m.grow(1);return [r,b.byteLength,e.m.buffer.byteLength,b===e.m.buffer,e.m.buffer===e.m.buffer]})`);
add(`T(()=>{var e=${instOf("sec(5,[1,1,1,1]),ES(ex('m',2,0))")};return e.m.grow(1)})`);
add(`T(()=>{var e=${instOf("sec(5,[1,3,1,2]),ES(ex('m',2,0))")};return [e.m.buffer instanceof SharedArrayBuffer,Object.isFrozen(e.m.buffer)]})`);
add(`T(()=>{var e=${instOf("TY(ft([],[]),ft([0x7f],[])),sec(13,[1,0,1]),ES(ex('t',4,0))".replace("I32", "0x7f"))};return [Object.prototype.toString.call(e.t),e.t instanceof WebAssembly.Tag,Object.getPrototypeOf(e.t)===WebAssembly.Tag.prototype]})`);
add(`T(()=>{var e=${instOf("TY(ft([],[])),sec(13,[1,0,0]),ES(ex('t',4,0))")};return e.t===e.t})`);
add(`T(()=>{var e=${instOf("TY(ft([],[])),sec(13,[1,0,0]),ES(ex('a',4,0),ex('b',4,0))")};return e.a===e.b})`);
add(`T(()=>{var e=${instOf("TY(ft([],[])),FS(0),ES(ex('a',0,0),ex('b',0,0)),CS(Fn([]))")};return [e.a===e.b,e.a.name,e.b.name]})`);
add(`T(()=>{var e=${instOf("sec(6,[2,0x7f,0,0x41,1,0x0b,0x7f,0,0x41,1,0x0b]),ES(ex('a',3,0),ex('b',3,1))")};return e.a===e.b})`);
add(`T(()=>{var e=${instOf("sec(6,[1,0x7f,0,0x41,1,0x0b]),ES(ex('a',3,0),ex('b',3,0))")};return e.a===e.b})`);
add(`T(()=>{var e=${instOf("IS(im('m','g',[3,0x7f,0])),ES(ex('g',3,0))", "{m:{g:9}}")};return [P(e.g.value)]})`);
add(`T(()=>{var G=new WebAssembly.Global({value:'i32',mutable:true},3);var e=${instOf("IS(im('m','g',[3,0x7f,1])),ES(ex('g',3,0))", "{m:{g:G}}")};return [e.g===G,P(e.g.value)]})`);
add(`T(()=>{var G=new WebAssembly.Global({value:'i32'},3);var e=${instOf("IS(im('m','g',[3,0x7f,0])),ES(ex('g',3,0))", "{m:{g:G}}")};return e.g===G})`);
add(`T(()=>{var M1=new WebAssembly.Memory({initial:1});var e=${instOf("IS(im('m','x',[2,0,1])),ES(ex('x',2,0))", "{m:{x:M1}}")};return e.x===M1})`);
add(`T(()=>{var T1=new WebAssembly.Table({element:'anyfunc',initial:1});var e=${instOf("IS(im('m','x',[1,0x70,0,1])),ES(ex('x',1,0))", "{m:{x:T1}}")};return e.x===T1})`);
add(`T(()=>{var T1=new WebAssembly.Tag({parameters:[]});var e=${instOf("TY(ft([],[])),IS(im('m','x',[4,0,0])),ES(ex('x',4,0))", "{m:{x:T1}}")};return e.x===T1})`);
add(`T(()=>{var f=()=>1;var e=${instOf("TY(ft([],[0x7f])),IS(im('m','x',[0,0])),ES(ex('x',0,0))", "{m:{x:f}}")};return [e.x===f,typeof e.x,e.x.name,e.x.length]})`);
add(`T(()=>{var e=${instOf("TY(ft([],[0x7f])),IS(im('m','x',[0,0])),ES(ex('x',0,0))", "{m:{x:()=>5}}")};return e.x()})`);

// ---- 12. API de Global.
const gdescs = ["{value:'i32'}", "{value:'i64'}", "{value:'f32'}", "{value:'f64'}", "{value:'v128'}", "{value:'externref'}", "{value:'anyfunc'}", "{value:'funcref'}", "{value:'eqref'}", "{value:'i31ref'}",
  "{value:'structref'}", "{value:'arrayref'}", "{value:'anyref'}", "{value:'nullref'}", "{value:'i8'}", "{value:'i16'}", "{value:'bool'}", "{value:''}", "{value:'I32'}", "{value:undefined}", "{value:null}",
  "{value:1}", "{}", "{mutable:true}", "undefined", "null", "1", "'i32'", "{value:'i32',mutable:true}", "{value:'i32',mutable:false}", "{value:'i32',mutable:1}", "{value:'i32',mutable:'x'}",
  "{value:'i32',mutable:undefined}", "{value:'i32',mutable:null}", "{value:'i32',mutable:0}", "{value:{toString(){return 'f32'}}}", "{get value(){return 'i64'}}", "{value:'i64',mutable:true}"];
const gvals = ["", ",undefined", ",null", ",0", ",-0", ",1", ",1.5", ",-1", ",NaN", ",Infinity", ",2**31", ",2**32", ",2**53", ",'7'", ",'x'", ",true", ",{}", ",[]", ",0n", ",1n", ",2n**63n", ",2n**64n", ",-(2n**63n)",
  ",Symbol()", ",()=>1", ",{valueOf(){return 4}}", ",{valueOf(){throw new RangeError('v')}}", ",1e40", ",16777217", ",0.1", ",3.4028236e38", ",5e-324"];
for (const d of gdescs) add(`T(()=>{var g=new WebAssembly.Global(${d});return [P(g.value),g.valueOf===undefined]})`);
for (const d of ["{value:'i32'}", "{value:'i64'}", "{value:'f32'}", "{value:'f64'}", "{value:'externref'}", "{value:'anyfunc'}", "{value:'v128'}", "{value:'i32',mutable:true}", "{value:'i64',mutable:true}", "{value:'f32',mutable:true}", "{value:'f64',mutable:true}"]) {
  for (const v of gvals) add(`T(()=>{var g=new WebAssembly.Global(${d}${v});return P(g.value)})`);
}
for (const d of ["{value:'i32',mutable:true}", "{value:'i64',mutable:true}", "{value:'f32',mutable:true}", "{value:'f64',mutable:true}", "{value:'externref',mutable:true}", "{value:'anyfunc',mutable:true}", "{value:'i32'}", "{value:'i64'}", "{value:'f32'}", "{value:'externref'}", "{value:'v128',mutable:true}"]) {
  for (const v of ["undefined", "null", "1", "1.5", "'x'", "{}", "1n", "2n**64n", "NaN", "-0", "2**32", "Symbol()", "true", "()=>1"]) {
    add(`T(()=>{var g=new WebAssembly.Global(${d});g.value=${v};return P(g.value)})`);
  }
  add(`T(()=>{var g=new WebAssembly.Global(${d});return [g.valueOf===WebAssembly.Global.prototype.valueOf,P(g.valueOf()),Object.getOwnPropertyNames(g).length]})`);
}
add("T(()=>WebAssembly.Global({value:'i32'}))", "T(()=>new WebAssembly.Global())", "T(()=>WebAssembly.Global.prototype.value)", "T(()=>WebAssembly.Global.prototype.valueOf())",
  "T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype,'value').get.call({}))", "T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype,'value').set.call({},1))",
  "T(()=>Object.getOwnPropertyNames(WebAssembly.Global.prototype).join())", "T(()=>WebAssembly.Global.length)", "T(()=>WebAssembly.Global.name)",
  "T(()=>{var g=new WebAssembly.Global({value:'i32'});g.value=1})", "T(()=>{'use strict';var g=new WebAssembly.Global({value:'i32'});g.value=1})",
  "T(()=>{var g=new WebAssembly.Global({value:'i32'},1);try{g.value=2}catch(e){return e.message}})", "T(()=>{var g=new WebAssembly.Global({value:'i64'},1n);return typeof g.value})",
  "T(()=>{var g=new WebAssembly.Global({value:'i64'});return P(g.value)})", "T(()=>{var g=new WebAssembly.Global({value:'f32'},0.1);return P(g.value)})",
  "T(()=>{var g=new WebAssembly.Global({value:'externref'});return P(g.value)})", "T(()=>{var g=new WebAssembly.Global({value:'anyfunc'});return P(g.value)})",
  "T(()=>{var g=new WebAssembly.Global({value:'anyfunc'},()=>1);return P(g.value)})", "T(()=>{var g=new WebAssembly.Global({value:'anyfunc'},{});return P(g.value)})",
  "T(()=>{var g=new WebAssembly.Global({value:'anyfunc'},undefined);return P(g.value)})", "T(()=>{var g=new WebAssembly.Global({value:'anyfunc',mutable:true});g.value=()=>1})",
  "T(()=>{var g=new WebAssembly.Global({value:'v128'});return 1})", "T(()=>{var g=new WebAssembly.Global({value:'v128'},1)})", "T(()=>{var g=new WebAssembly.Global({value:'v128',mutable:true})})");

// ---- 13. API de Table.
const tdescs = ["{element:'anyfunc',initial:1}", "{element:'funcref',initial:1}", "{element:'externref',initial:1}", "{element:'i32',initial:1}", "{element:'anyref',initial:1}", "{element:'eqref',initial:1}", "{element:'',initial:1}",
  "{element:'anyfunc'}", "{initial:1}", "{element:'anyfunc',initial:0}", "{element:'anyfunc',initial:-1}", "{element:'anyfunc',initial:1.5}", "{element:'anyfunc',initial:'2'}", "{element:'anyfunc',initial:2**32}",
  "{element:'anyfunc',initial:10000000}", "{element:'anyfunc',initial:10000001}", "{element:'anyfunc',initial:1,maximum:0}", "{element:'anyfunc',initial:1,maximum:1}", "{element:'anyfunc',initial:1,maximum:2**32-1}",
  "{element:'anyfunc',initial:1,maximum:2**32}", "{element:'anyfunc',initial:1,maximum:-1}", "{element:'anyfunc',initial:1,maximum:undefined}", "{element:'anyfunc',initial:1,maximum:null}",
  "{element:'anyfunc',minimum:1}", "{element:'anyfunc',minimum:1,initial:1}", "{element:'anyfunc',minimum:2,initial:1}", "{element:'anyfunc',minimum:1,maximum:3}", "{element:'anyfunc',initial:NaN}", "{element:'anyfunc',initial:Infinity}",
  "{element:'anyfunc',initial:{valueOf(){return 3}}}", "{element:'anyfunc',initial:1n}", "{element:'anyfunc',initial:true}", "{element:'anyfunc',initial:undefined}", "{element:'anyfunc',initial:null}", "{element:{toString(){return 'anyfunc'}},initial:1}",
  "{element:'ANYFUNC',initial:1}", "undefined", "null", "1", "'x'", "[]", "{element:'externref',initial:2,maximum:1}", "{element:'externref',initial:0,maximum:0}"];
for (const d of tdescs) {
  add(`T(()=>{var t=new WebAssembly.Table(${d});return [t.length,t.get(0)===undefined?'u':String(t.get(0))]})`);
  add(`T(()=>{var t=new WebAssembly.Table(${d},{a:1});return [t.length,typeof t.get(0)]})`);
}
for (const el of ["'anyfunc'", "'externref'"]) {
  for (const iv of ["", ",undefined", ",null", ",1", ",{}", ",()=>1", ",'s'", ",1n", ",Symbol()"]) {
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2}${iv});return [typeof t.get(0),typeof t.get(1),t.get(0)===t.get(1)]})`);
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:1,maximum:3});t.grow(1${iv});return [t.length,typeof t.get(1)]})`);
  }
  for (const idx of ["0", "1", "2", "-1", "1.5", "'0'", "'x'", "undefined", "null", "2**32", "2**32-1", "NaN", "Infinity", "{valueOf(){return 1}}", "1n", "true"]) {
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});return String(t.get(${idx}))})`);
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});t.set(${idx},null);return t.length})`);
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2,maximum:4});return t.grow(${idx})+','+t.length})`);
  }
  for (const v of ["undefined", "null", "1", "'s'", "{}", "()=>1", "Math.max", "1n", "Symbol()", "true", "new WebAssembly.Global({value:'i32'})"]) {
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});t.set(0,${v});return typeof t.get(0)})`);
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});t.set(0,${v});t.set(1,t.get(0));return t.get(0)===t.get(1)})`);
    add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:1});return t.grow(1,${v})})`);
  }
  add(`T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});return t.set(0)})`, `T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});return t.set()})`,
    `T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});return t.get()})`, `T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});return t.grow()})`,
    `T(()=>{var t=new WebAssembly.Table({element:${el},initial:2});return t.set(0,null)})`, `T(()=>{var t=new WebAssembly.Table({element:${el},initial:1,maximum:1});return t.grow(1)})`,
    `T(()=>{var t=new WebAssembly.Table({element:${el},initial:1,maximum:1});return t.grow(0)})`, `T(()=>{var t=new WebAssembly.Table({element:${el},initial:1});return t.grow(10000000)})`,
    `T(()=>{var t=new WebAssembly.Table({element:${el},initial:0});return t.grow(10000000)})`, `T(()=>{var t=new WebAssembly.Table({element:${el},initial:0});return t.grow(10000001)})`);
}
add("T(()=>WebAssembly.Table({element:'anyfunc',initial:1}))", "T(()=>new WebAssembly.Table())", "T(()=>WebAssembly.Table.prototype.length)", "T(()=>WebAssembly.Table.prototype.get.call({},0))",
  "T(()=>WebAssembly.Table.prototype.set.call({},0))", "T(()=>WebAssembly.Table.prototype.grow.call({},0))", "T(()=>Object.getOwnPropertyNames(WebAssembly.Table.prototype).join())",
  "T(()=>[WebAssembly.Table.length,WebAssembly.Table.prototype.get.length,WebAssembly.Table.prototype.set.length,WebAssembly.Table.prototype.grow.length])",
  "T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Table.prototype,'length').get.call({}))",
  "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});t.set(0,()=>1)})", "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});t.set(0,{})})",
  "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});t.set(0,undefined);return String(t.get(0))})", "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});t.set(0,null);return String(t.get(0))})",
  "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1},()=>1)})", "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});t.grow(1,()=>1)})",
  "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1});var e=new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M(TY(ft([],[0x7f])),FS(0),ES(ex('f',0,0)),CS(Fn([0x41,9]))))));t.set(0,e.exports.f);return [t.get(0)===e.exports.f,t.get(0)()]})",
  "T(()=>{var t=new WebAssembly.Table({element:'anyfunc',initial:1},new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M(TY(ft([],[0x7f])),FS(0),ES(ex('f',0,0)),CS(Fn([0x41,9])))))).exports.f);return t.get(0)()})");

// ---- 14. Memory e Tag, API básica.
for (const d of ["{initial:0}", "{initial:1}", "{initial:65536}", "{initial:65537}", "{initial:1,maximum:0}", "{initial:1,maximum:65536}", "{initial:1,maximum:65537}", "{initial:2,maximum:1}", "{minimum:1}", "{minimum:1,initial:1}",
  "{minimum:2,initial:1}", "{minimum:1,maximum:2}", "{initial:1,shared:true}", "{initial:1,maximum:2,shared:true}", "{initial:1,maximum:2,shared:false}", "{initial:-1}", "{initial:1.5}", "{initial:'1'}", "{initial:NaN}",
  "{initial:2**32}", "{}", "undefined", "null", "1", "{initial:1,maximum:undefined}", "{initial:1,maximum:null}", "{initial:1,shared:1}", "{initial:1,shared:'x'}", "{initial:{valueOf(){return 1}}}", "{initial:1n}", "{initial:true}",
  "{initial:1,index:'i64'}", "{initial:1,index:'i32'}", "{initial:1,index:'x'}", "{initial:1,address:'i32'}", "{initial:1,address:'i64'}"]) {
  add(`T(()=>{var m=new WebAssembly.Memory(${d});return [m.buffer.byteLength,m.buffer.constructor.name,m.buffer.resizable,m.buffer.maxByteLength]})`);
  add(`T(()=>{var m=new WebAssembly.Memory(${d});return [m.grow(1),m.buffer.byteLength]})`);
  add(`T(()=>{var m=new WebAssembly.Memory(${d});return [m.grow(0),m.buffer.byteLength]})`);
}
for (const g of ["0", "1", "-1", "1.5", "'1'", "undefined", "null", "NaN", "65535", "65536", "65537", "2**32", "2**32-1", "{valueOf(){return 1}}", "1n", "true", "Symbol()"]) {
  add(`T(()=>{var m=new WebAssembly.Memory({initial:1,maximum:100});return [m.grow(${g}),m.buffer.byteLength]})`);
  add(`T(()=>{var m=new WebAssembly.Memory({initial:1,maximum:3,shared:true});return [m.grow(${g}),m.buffer.byteLength,m.buffer.growable]})`);
}
add("T(()=>WebAssembly.Memory({initial:1}))", "T(()=>new WebAssembly.Memory())", "T(()=>WebAssembly.Memory.prototype.buffer)", "T(()=>WebAssembly.Memory.prototype.grow.call({},1))",
  "T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Memory.prototype,'buffer').get.call({}))", "T(()=>Object.getOwnPropertyNames(WebAssembly.Memory.prototype).join())",
  "T(()=>{var m=new WebAssembly.Memory({initial:1});var b=m.buffer;m.grow(0);return [b===m.buffer,b.byteLength]})", "T(()=>{var m=new WebAssembly.Memory({initial:1});var b=m.buffer;m.grow(1);return [b===m.buffer,b.byteLength,b.detached]})",
  "T(()=>{var m=new WebAssembly.Memory({initial:1});return Object.isFrozen(m.buffer)+','+Object.isExtensible(m.buffer)})", "T(()=>{var m=new WebAssembly.Memory({initial:1});m.buffer.transfer()})",
  "T(()=>{var m=new WebAssembly.Memory({initial:1});m.buffer.resize(1)})", "T(()=>{var m=new WebAssembly.Memory({initial:1});return structuredClone===undefined})");
for (const d of ["{parameters:[]}", "{parameters:['i32']}", "{parameters:['i64','f32','f64']}", "{parameters:['externref']}", "{parameters:['anyfunc']}", "{parameters:['v128']}", "{parameters:['x']}", "{parameters:[1]}", "{parameters:'i32'}",
  "{parameters:{length:1,0:'i32'}}", "{parameters:null}", "{parameters:undefined}", "{}", "undefined", "null", "1", "{parameters:[],results:[]}", "{parameters:[],results:['i32']}", "{parameters:new Set(['i32'])}",
  "{parameters:['i32','i32','i32','i32','i32','i32','i32','i32','i32','i32']}", "{parameters:{[Symbol.iterator]:function*(){yield 'f32'}}}", "{parameters:['I32']}", "{parameters:['']}"]) {
  add(`T(()=>{var t=new WebAssembly.Tag(${d});return [Object.prototype.toString.call(t),Object.getOwnPropertyNames(t).length]})`);
}
add("T(()=>WebAssembly.Tag({parameters:[]}))", "T(()=>new WebAssembly.Tag())", "T(()=>Object.getOwnPropertyNames(WebAssembly.Tag.prototype).join())", "T(()=>WebAssembly.Tag.length)", "T(()=>WebAssembly.Tag.prototype.x)");
// Instance, Module (construção).
for (const v of ["undefined", "null", "1", "'abc'", "{}", "[]", "[0,97,115,109,1,0,0,0]", "new Uint8Array(0)", "new Uint8Array(M())", "new Uint8Array(M()).buffer", "new Uint8Array(M()).subarray(1)", "new Uint8Array(M()).subarray(0,7)",
  "new Uint16Array(M())", "new Uint32Array([1836278016,1])", "new Uint8Array(M()).buffer.slice(0,4)", "new DataView(new Uint8Array(M()).buffer)", "new Float32Array(2)", "new Int8Array(M())", "new Uint8ClampedArray(M())",
  "new ArrayBuffer(0)", "new ArrayBuffer(8)", "Object.create(null)", "new Proxy(new Uint8Array(M()),{})", "new BigInt64Array(1)", "new SharedArrayBuffer(8)", "new Uint8Array(new SharedArrayBuffer(8))", "new Uint8Array(new ArrayBuffer(16,{maxByteLength:32}))",
  "(()=>{var b=new Uint8Array(M());b.buffer.transfer();return b})()", "(()=>{var a=new Uint8Array(M(TY(ft([],[]))));return a})()", "new Uint8Array([0,97,115,109,1,0,0,0,0])", "new Uint8Array([0,97,115,109,1,0,0,0,0,0])"]) {
  add(`C(${v})`.replace("C(", "T(()=>{new WebAssembly.Module(").replace(/\)$/, ");return 'ok'})"));
  add(`T(()=>WebAssembly.validate(${v}))`);
}
add("T(()=>new WebAssembly.Module())", "T(()=>WebAssembly.Module(new Uint8Array(M())))", "T(()=>WebAssembly.Module.prototype.x)", "T(()=>Object.getOwnPropertyNames(WebAssembly.Module).join())",
  "T(()=>WebAssembly.Module.length)", "T(()=>WebAssembly.Instance.length)", "T(()=>new WebAssembly.Instance())", "T(()=>new WebAssembly.Instance({}))", "T(()=>new WebAssembly.Instance(1))",
  "T(()=>new WebAssembly.Instance(new Uint8Array(M())))", "T(()=>WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M()))))", "T(()=>Object.getOwnPropertyNames(WebAssembly.Instance.prototype).join())",
  "T(()=>{var i=new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M())));return [Object.getOwnPropertyNames(i).length,Object.getOwnPropertyDescriptor(WebAssembly.Instance.prototype,'exports').get.call(i)===i.exports]})",
  "T(()=>Object.getOwnPropertyDescriptor(WebAssembly.Instance.prototype,'exports').get.call({}))", "T(()=>{var i=new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(M())));return [Object.isFrozen(i.exports),Object.getPrototypeOf(i.exports),i.exports===i.exports]})");

// ---- Dedupe com os goldens existentes e geração.
const existing = [];
for (const file of fs.readdirSync(path.join(__dirname, "..", "tests", "golden"))) {
  if (/^wasm_.*\.tsv$/.test(file) && file !== "wasm_module_bun.tsv") {
    existing.push(fs.readFileSync(path.join(__dirname, "..", "tests", "golden", file), "utf8"));
  }
}
const baseText = existing.join("\n");
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0, dropped = 0, dup = 0;
const jobs = [];
for (const expr of unique) {
  if (expr.length > 24 && baseText.includes(JSON.stringify(expr).slice(1, -1))) { dup++; continue; }
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${/^T\(/.test(expr) ? expr : /^[CEI]\(/.test(expr) ? expr : "T(()=>" + expr + ")"}`;
  jobs.push({ expr, source });
}
for (const { expr, source } of jobs) {
  let result;
  try {
    const child = spawnSync(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { input: source, encoding: "utf8", maxBuffer: 1 << 26, timeout: 20000 });
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
  kept++;
  rows.push({ source, result });
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
process.stdout.write(emitFactored("wasm_module", rows));
