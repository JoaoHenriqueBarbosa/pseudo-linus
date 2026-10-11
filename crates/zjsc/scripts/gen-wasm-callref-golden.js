// Gera tests/golden/wasm_callref_bun.tsv: `call_ref` e `return_call_ref` (typed function references) em módulos montados
// por um mini-assembler, mais a ausência de `type()` (type reflection) em Memory, Table, Global e Tag, que o bun 1.4.2
// não expõe. Cada programa grava em `R` um texto; o bun roda cada um em processo próprio. Colunas: fonte, resultado.
// Uso:
//   bun scripts/gen-wasm-callref-golden.js > tests/golden/wasm_callref_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const leb = (n) => {
  const out = [];
  do {
    let byte = n & 0x7f;
    n >>>= 7;
    if (n) byte |= 0x80;
    out.push(byte);
  } while (n);
  return out;
};
const sleb = (n) => {
  const out = [];
  for (;;) {
    const byte = n & 0x7f;
    n >>= 7;
    if ((n === 0 && !(byte & 0x40)) || (n === -1 && byte & 0x40)) {
      out.push(byte);
      return out;
    }
    out.push(byte | 0x80);
  }
};
const vec = (items) => [...leb(items.length), ...items.flat()];
const name = (text) => [...leb(text.length), ...Buffer.from(text)];
const section = (id, body) => [id, ...leb(body.length), ...body];
const I32 = 0x7f;

// types: [[params], [results]]; imports: [{name, type}] (módulo "m"); funcs: índices de tipo; exports: [{name, func}];
// declared: índices de função que o `ref.func` referencia; bodies: ops de cada função (sem locals nem `end`).
const assemble = ({ types, imports = [], funcs, exports = [], declared = [], bodies }) => {
  const bytes = [0, 0x61, 0x73, 0x6d, 1, 0, 0, 0];
  bytes.push(...section(1, vec(types.map(([params, results]) => [0x60, ...vec(params.map((p) => [p])), ...vec(results.map((r) => [r]))]))));
  if (imports.length) bytes.push(...section(2, vec(imports.map((i) => [...name("m"), ...name(i.name), 0, ...leb(i.type)]))));
  bytes.push(...section(3, vec(funcs.map((t) => leb(t)))));
  if (exports.length) bytes.push(...section(7, vec(exports.map((e) => [...name(e.name), 0, ...leb(e.func)]))));
  if (declared.length) bytes.push(...section(9, [1, 3, 0, ...vec(declared.map((f) => leb(f)))]));
  bytes.push(...section(10, vec(bodies.map((ops) => { const body = [0, ...ops.flat(), 0x0b]; return [...leb(body.length), ...body]; }))));
  return bytes;
};
const op = {
  i32: (n) => [0x41, ...sleb(n)],
  get: (i) => [0x20, ...leb(i)],
  add: [0x6a],
  refFunc: (f) => [0xd2, ...leb(f)],
  refNull: (t) => [0xd0, ...leb(t)],
  callRef: (t) => [0x14, ...leb(t)],
  returnCallRef: (t) => [0x15, ...leb(t)],
  callRefBad: [0x14],
};

const programs = [];
const seen = new Set();
const add = (...sources) => {
  for (const source of sources) {
    if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
    if (!seen.has(source)) {
      seen.add(source);
      programs.push(source);
    }
  }
};
const T = "var T=f=>{try{return String(f())}catch(e){return String(e)}};";
const program = (body) => `globalThis.R=(()=>{${T}var B=b=>new Uint8Array(b);var M=b=>new WebAssembly.Module(B(b));var I=(b,i)=>new WebAssembly.Instance(M(b),i);${body}})();`;
const J = JSON.stringify;

// $f: () -> i32 devolvendo 1; t chama por call_ref.
const simple = assemble({
  types: [[[], [I32]]],
  funcs: [0, 0],
  exports: [{ name: "t", func: 1 }],
  declared: [0],
  bodies: [[op.i32(1)], [op.refFunc(0), op.callRef(0)]],
});
// $add: (i32, i32) -> i32; argumentos ficam abaixo da referência.
const withArgs = assemble({
  types: [[[I32, I32], [I32]], [[I32, I32], [I32]]],
  funcs: [0, 1],
  exports: [{ name: "t", func: 1 }],
  declared: [0],
  bodies: [[op.get(0), op.get(1), op.add], [op.get(0), op.get(1), op.refFunc(0), op.callRef(0)]],
});
// call_ref de uma referência nula.
const nullRef = assemble({
  types: [[[], [I32]]],
  funcs: [0],
  exports: [{ name: "t", func: 0 }],
  bodies: [[op.refNull(0), op.callRef(0)]],
});
// return_call_ref.
const tail = assemble({
  types: [[[], [I32]]],
  funcs: [0, 0],
  exports: [{ name: "t", func: 1 }],
  declared: [0],
  bodies: [[op.i32(7)], [op.refFunc(0), op.returnCallRef(0)]],
});
// ref.func de um import: a chamada cai no JS e o resultado volta pela assinatura.
const imported = assemble({
  types: [[[], [I32]]],
  imports: [{ name: "f", type: 0 }],
  funcs: [0],
  exports: [{ name: "t", func: 1 }],
  declared: [0],
  bodies: [[op.refFunc(0), op.callRef(0)]],
});
// ref.func de uma função exportada por outra instância, chamada pelo importador.
const crossProvider = assemble({
  types: [[[], [I32]]],
  funcs: [0],
  exports: [{ name: "f", func: 0 }],
  bodies: [[op.i32(42)]],
});
// call_ref com tipo de operando errado (i32 no lugar da referência): não valida.
const invalid = assemble({
  types: [[[], [I32]]],
  funcs: [0],
  bodies: [[op.i32(0), op.callRef(0)]],
});
// índice de tipo fora do limite.
const outOfBounds = assemble({
  types: [[[], [I32]]],
  funcs: [0],
  bodies: [[op.refNull(0), op.callRef(5)]],
});

add(
  program(`return T(()=>I(${J(simple)}).exports.t())`),
  program(`return T(()=>I(${J(withArgs)}).exports.t(20,22))`),
  program(`return T(()=>I(${J(nullRef)}).exports.t())`),
  program(`return T(()=>I(${J(tail)}).exports.t())`),
  program(`return T(()=>I(${J(imported)},{m:{f:()=>9}}).exports.t())`),
  program(`var p=I(${J(crossProvider)});return T(()=>I(${J(imported)},{m:{f:p.exports.f}}).exports.t())`),
  program(`return WebAssembly.validate(B(${J(simple)}))+"|"+WebAssembly.validate(B(${J(withArgs)}))+"|"+WebAssembly.validate(B(${J(tail)}))`),
  program(`return WebAssembly.validate(B(${J(invalid)}))+"|"+T(()=>M(${J(invalid)}))`),
  program(`return WebAssembly.validate(B(${J(outOfBounds)}))+"|"+T(()=>M(${J(outOfBounds)}))`),
  // Type reflection: o bun não expõe `type` em nenhum dos quatro.
  program(`return typeof new WebAssembly.Memory({initial:1}).type+"|"+typeof new WebAssembly.Table({element:"anyfunc",initial:1}).type+"|"+typeof new WebAssembly.Global({value:"i32"}).type+"|"+typeof WebAssembly.Tag.prototype.type`),
  program(`return ("type" in WebAssembly.Memory.prototype)+"|"+("type" in WebAssembly.Table.prototype)+"|"+("type" in WebAssembly.Global.prototype)+"|"+("type" in WebAssembly.Tag.prototype)`),
  program(`return [WebAssembly.Memory,WebAssembly.Table,WebAssembly.Global,WebAssembly.Tag].map(c=>Object.getOwnPropertyNames(c.prototype).join()).join("|")`),
  program(`return T(()=>new WebAssembly.Tag({parameters:["i32"]}).type())`)
);

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-callref-golden-"));
const lines = [];
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  fs.writeFileSync(file, `${source}\nprocess.stdout.write(String(globalThis.R));\n`);
  const run = spawnSync(process.execPath, [file], { timeout: 10000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = "sem resultado do bun";
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
