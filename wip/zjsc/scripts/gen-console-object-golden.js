// Gera tests/golden/console_object_bun.tsv: `console.log` de arrays e objetos simples (literais, aninhados, vazios, chaves que
// pedem aspas, símbolos como chave, buracos de array, profundidade máxima, quebra de linha acima da largura estimada,
// circular), medidos no bun 1.4.2. Colunas (todas em JSON): a fonte do programa, os bytes do stdout em hex, os bytes do
// stderr em hex e o valor da variável global `R` (a exceção, como `Nome|mensagem`, ou `<undefined>`).
// Os casos aleatórios vêm de um gerador congruencial fixo: a saída é determinística.
// Uso: bun scripts/gen-console-object-golden.js > tests/golden/console_object_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const { inspectArgs } = require("./console-object-model.js");

const programs = [];
const run = (code, random = false) => programs.push({ code, random });

// Casos fixos: um por regra medida.
const fixed = [
  "{}", "[]", "{a:1}", '{a:1,b:"x"}', '[1,"a",null,undefined]', "[[]]", "[{}]", "[[1]]", "[1,[2]]", "[1,{a:1}]", "[[1],1]", "[{a:1},1]",
  "{a:[1,2]}", "{a:{}}", "{a:[]}", "{a:{b:1}}", "{a:[{b:1}]}", "{a:[[1]]}",
  '{"a-b":1,"1a":2,3:4,"":5,"é":6,$x:1,_y:2}', '{"key with space":1,"a.b":2,a1:3,"ab":4,"0":5,"01":6,"-1":7,"1.5":8}', "{0:1,1:2}", "{b:1,a:2,1:3}",
  '{"a\\nb":1,"q\\"":2,"a b":4}',
  '{[Symbol("s")]:1,[Symbol()]:2,[Symbol.iterator]:3}', '{[Symbol("a b")]:1,[Symbol.for("x")]:2,[Symbol("")]:3}',
  "[1,,3]", "[,,]", "[1,,,,5]", "new Array(5)", "[,1]", "[,]", "[1,,]", "[,,1,,]",
  "{a:{b:{c:{d:1}}}}", "{a:{b:{c:{}}}}", "{a:{b:{c:[]}}}", "{a:{b:{c:[1]}}}", "[[[[1]]]]", "[[[[[1]]]]]", "[[[[[[1]]]]]]", "[{a:{b:{c:{d:1}}}}]",
  "{a:{b:{c:[[1]]}}}", "{a:{b:[{c:1}]}}", "{a:[[[[1]]]]}", "{a:[[[[[1]]]]]}", "[1,[2,[3,[4,[5]]]]]",
  "Array.from({length:5},(_, i)=>i)", "Array.from({length:10},(_, i)=>i)", "Array.from({length:11},(_, i)=>i)", "Array.from({length:30},(_, i)=>i)",
  "Array.from({length:120},(_, i)=>i)", 'Array.from({length:30},(_, i)=>"item"+i)', 'Array.from({length:10},(_, i)=>"item"+i)', 'Array.from({length:11},(_, i)=>"item"+i)',
  '["a\\nb","t\\tt","q\\"q","b\\\\s","\\r","\\u0001","\\u007f","é","日本","a`b","\\u001b[0m","\\u0000"]',
  '{a:"x".repeat(100)}', '["x".repeat(100)]', '["x".repeat(100),"y"]', '["x".repeat(50),"y".repeat(50),"z"]', '["x".repeat(50),"y".repeat(50),1]',
  '["aaaaaaaaaaaaaaaaaaaa","bbbbbbbbbbbbbbbbbbbbbbbb","cccccccccccccccccccccccc","dddddddddddddddddddd"]',
  "[true,false,null,undefined,1n,Symbol('a')]", "[1.5,-0,NaN,10n,true,Symbol('z')]", "[undefined,undefined]", "{a:undefined,b:null}",
  "Object.create(null)", "Object.assign(Object.create(null),{a:1})",
  "Object.assign([1,2],{k:1})", "(()=>{const y=[1,2];y[-1]=3;return y})()", "(()=>{const h=[1,2,3];h[10]=4;return h})()", "(()=>{const e=[];e[3]=1;return e})()",
  "[[1,2],[3,4]]", "[{a:1},{b:2}]", "[{a:1},{a:2},{a:3}]", "[[],[]]", "[1,[],2]", "[1,{},2]", '["a",{x:1},"b"]', "[{},{},{}]", "[[1],[2],[3]]",
  "{a:[1,2,{b:[3,{c:4}]}]}", "{a:1,b:{c:2}}", "{a:1,b:[1,2,3],c:'s'}", "{a:[1,2,3],b:[4,5,6]}",
  "Array.from({length:26},(_, i)=>[String.fromCharCode(97+i), i]).reduce((o, [k, v])=>(o[k]=v, o), {})",
  "[[1,2],[3,4],[5,6],[7,8],[9,10],[11,12],[13,14],[15,16],[17,18],[19,20],[21,22],[23,24],[25,26]]",
  "{aaaaaaaaaaaaaaaaaaaaaaaaaaaa:['x'.repeat(20),'y'.repeat(20),'z'.repeat(20),'w']}",
  "[ 'x'.repeat(30), {a:'y'.repeat(30), b:'z'.repeat(30)}, 'x'.repeat(30), 'x'.repeat(30)]",
  "[['x'.repeat(30),'y'.repeat(30)],['x'.repeat(30),'y'.repeat(30)],['x'.repeat(30),'y'.repeat(30)]]",
  "{a:{b:[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30]}}",
  "(()=>{const o={a:1};o.self=o;return o})()", "(()=>{const c={x:{y:1}};c.x.back=c;return c})()", "(()=>{const o={a:1};o.self=o;return [o]})()",
  "(()=>{const a=[1];a.push(a);return a})()", "(()=>{const a=[];a.push(a);return a})()", "(()=>{const o={};o.a=o;o.b=[o];return o})()",
];
for (const code of fixed) run(`console.log(${code})`);
run("console.log(1,[1,2],{a:1},'s',[3])");
run("console.log('s',{a:'x'.repeat(70)},[1,2,3])");
run("console.log('title',[1,2,3])");
run("console.log('title',{a:1})");
run("console.log({a:1},{b:2})");
run("console.log([1,2],[3,4])");
run("console.info({a:[1,2]})");
run("console.debug([{a:1}])");
run("console.error({a:1})");
run("console.warn([1,2,3])");
run("console.log('%s', {a:1})".replace("%s", "x"));
run("console.log('a', 'b', [1])");

// Casos aleatórios determinísticos.
let seed = 1;
const rnd = () => { seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
const pick = (a) => a[Math.floor(rnd() * a.length)];
const words = ["a", "bb", "key", "alpha", "x".repeat(15), "y".repeat(30), "item", "q r", "k-1", "$z"];
function gen(d) {
  const r = rnd();
  if (d > 3 || r < 0.4) {
    return pick([
      () => String(Math.floor(rnd() * 100000)),
      () => JSON.stringify(pick(words)),
      () => JSON.stringify(pick(words).repeat(1 + Math.floor(rnd() * 3))),
      () => "null", () => "true", () => "undefined", () => "1.5", () => "-0", () => "12n",
    ])();
  }
  if (r < 0.7) {
    const n = pick([0, 1, 2, 3, 4, 5, 7, 9, 11, 13]);
    const parts = Array.from({ length: n }, () => gen(d + 1));
    if (n > 2 && rnd() < 0.15) {
      const hole = Math.floor(rnd() * n);
      let body = parts.map((p, i) => (i === hole ? "" : p)).join(",");
      if (hole === n - 1) body += ",";
      return `[${body}]`;
    }
    if (rnd() < 0.04) return `Object.assign([${parts.join(",")}],{extra:${gen(d + 1)}})`;
    return `[${parts.join(",")}]`;
  }
  const n = pick([0, 1, 2, 3, 4, 6]);
  const used = new Set();
  const props = [];
  for (let i = 0; i < n; i++) {
    const key = pick(words) + (rnd() < 0.3 ? i : "");
    if (used.has(key)) continue;
    used.add(key);
    props.push(`${JSON.stringify(key)}:${gen(d + 1)}`);
  }
  if (rnd() < 0.04) props.push('[Symbol("s")]:1');
  return `{${props.join(",")}}`;
}
for (let i = 0; i < 700; i++) {
  const value = gen(0);
  run(rnd() < 0.15 ? `console.log(${JSON.stringify(pick(words))},${value})` : `console.log(${value})`, true);
}

const E = "var E = function (e) { return e.name + '|' + e.message };\n";
// O que o modelo de referência prevê para o programa (os argumentos de console.* capturados num console falso).
function predicted(code) {
  let args = [];
  const capture = (...received) => { args = received; };
  new Function("console", code)({ log: capture, info: capture, debug: capture, error: capture, warn: capture });
  return inspectArgs(args) + "\n";
}

let diverging = 0;
for (const { code, random } of programs) {
  const source = (E + `try { ${code} } catch (e) { R = E(e) }`).replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "co-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `(0, eval)("var R");\n(0, eval)(${JSON.stringify(source)});\nprocess.stderr.write("\\u0000R" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
  );
  const result = spawnSync(process.execPath, [file], { timeout: 15000, input: "" });
  fs.rmSync(dir, { recursive: true, force: true });
  if (result.status !== 0) throw new Error("bun falhou em: " + source + "\n" + result.stderr);
  const at = result.stderr.lastIndexOf(Buffer.from("\u0000R"));
  const actual = result.stdout.toString("utf8") + result.stderr.subarray(0, at).toString("utf8");
  if (predicted(code) !== actual) {
    diverging++;
    process.stderr.write("DIVERGE" + (random ? " (aleatório)" : " (fixo)") + ": " + code + "\n");
  }
  emitRow(
    [
      JSON.stringify(source),
      JSON.stringify(result.stdout.toString("hex")),
      JSON.stringify(result.stderr.subarray(0, at).toString("hex")),
      result.stderr.subarray(at + 2).toString("utf8"),
    ].join("\t"),
  );
}
process.stderr.write(`divergentes do modelo: ${diverging}\n`);
