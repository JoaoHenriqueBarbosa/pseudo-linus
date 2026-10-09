// Gera tests/golden/console_dir_bun.tsv: `console.dir` (com e sem opções depth/colors), `console.dirxml` e `console.trace`
// medidos no bun 1.4.2. Colunas (todas em JSON): a fonte do programa, os bytes do stdout em hex, os bytes do stderr em
// hex e o valor da variável global `R` (a exceção, como `Nome|mensagem`, ou `<undefined>`).
// Diferente do golden primitivo, o programa é o próprio arquivo (sem `eval`): `console.trace` mostra os frames, e o
// caminho do arquivo do bun vira `console_dir_case.js` (o teste em Rust avalia o programa com esse nome).
// Uso: bun scripts/gen-console-dir-golden.js > tests/golden/console_dir_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const programs = [];
const run = (code) => programs.push(`var E = function (e) { return e.name + '|' + e.message };\ntry {\n${code}\n} catch (e) { globalThis.R = E(e) }`);

const nested = "{a:{b:{c:{d:{e:1}}}}}";
const values = [
  '"s"', '""', "1", "-0", "NaN", "1n", "true", "null", "undefined", 'Symbol("s")', "[]", "{}", "[1,[2,[3,[4,[5]]]]]", nested, "{a:1,b:'x'}",
  "new Map([[1,{a:{b:{c:1}}}]])", "new Set([[1,[2,[3,[4]]]]])", "function f(){}", "class A{}", "new Error('boom')", "[1,2,3]",
  "{x:[{y:[{z:[1]}]}]}", "new Date(0)", "/re/g", "{s:'a\\nb'}", "[,1]", "Object.create(null)", "new (class Foo{ constructor(){this.a={b:{c:{d:1}}}} })",
];
for (const v of values) {
  run(`console.dir(${v})`);
  run(`console.dir(${v}, {})`);
  run(`console.dir(${v}, {depth:0})`);
  run(`console.dir(${v}, {depth:1})`);
  run(`console.dir(${v}, {depth:null})`);
  run(`console.dir(${v}, {colors:true})`);
  run(`console.dir(${v}, {colors:false})`);
  run(`console.dir(${v}, {colors:true, depth:0})`);
  run(`console.dirxml(${v})`);
}
run("console.dir()");
run("console.dirxml()");
run("console.dir(1, 2, 3)");
run('console.dir("%s", "x")');
run('console.dir("a", "b")');
run("console.dir({a:1}, null)");
run("console.dir({a:1}, undefined)");
run("console.dir({a:1}, 5)");
run("console.dir({a:1}, 'str')");
run("console.dir({a:1}, true)");
run("console.dir({a:1}, [])");
run("console.dir({a:1}, function(){})");
run("console.dir(1, {depth:0}, 'extra')");
run("console.dir({a:1}, {depth:0}, {a:2})");
for (const d of ["0", "1", "2", "3", "-1", "-5", "1.9", "2.5", "NaN", "Infinity", "-Infinity", "null", "undefined", "'1'", "'x'", "true", "false", "{}", "[]", "[1]", "5n", "0n", "-1n", "2**31", "2**32", "2**32+1", "65535", "65536", "65537", "1e10", "-0", "Symbol()", "{valueOf(){return 1}}"]) {
  run(`console.dir(${nested}, {depth:${d}})`);
}
for (const c of ["true", "false", "1", "0", "'true'", "null", "undefined", "{}", "[]"]) {
  run(`console.dir({a:1,b:'s',c:null,d:undefined,e:[1,2]}, {colors:${c}})`);
  run(`console.dir(5, {colors:${c}})`);
}
// Erro com propriedades coloridas (cause, code, extras) e linha de fonte com cara de segredo (o bun não redige aqui).
for (const c of ["true", "false"]) {
  run(`const token = "npm_abcdefghijklmnopqrstuvwxyz0123456789abcd"; const e = new Error("x", { cause: new TypeError("inner") }); e.code = "E_X"; e.extra = { n: 1, s: "t", u: undefined }; e.password = "hunter2"; console.dir(e, {colors:${c}})`);
  run(`const e = new RangeError("r"); e.code = 42; e.list = [1, "a", null, true]; e.sym = Symbol("q"); e.big = 10n; console.dir(e, {colors:${c}})`);
  run(`const e = new Error("outer", { cause: { password: "hunter2", token: "abc" } }); console.dir(e, {colors:${c}})`);
}
run("console.dir({a:1}, {get depth(){ throw new Error('g') }})");
run("console.dir({a:1}, {get colors(){ throw new TypeError('c') }})");
run("console.dir({a:1}, {get depth(){ return 0 }, get colors(){ return true }})");
run("console.dir({a:1}, new Proxy({}, { get(t, k){ throw new RangeError('p' + String(k)) } }))");
run("console.dir(Symbol('s'), {depth:0})");
run("console.dir({toString(){ throw new Error('ts') }})");
run("console.dir({get a(){ throw new Error('ga') }})");
run("console.dirxml({a:1}, [2], 'x')");
run("console.dirxml('%s', 'x')");
run("console.dirxml(1, 2, 3)");
run("console.dirxml(Symbol('a'), 1)");
run("console.dirxml('[%s]', Symbol('s'))");
run("console.dir('[%s]', {depth:0})");
run("console.dirxml(null, undefined)");
run("console.dirxml('a\\nb')");
run("console.dirxml({a:1}, {b:2})");
run("console.dirxml({a:{b:{c:{d:1}}}}, 1)");
run("console.dir(null)");
run("console.dir(undefined)");
run("console.dir(null, {depth:0})");
run('console.group("g"); console.dir({a:{b:1}}); console.dir("s"); console.dirxml({a:1}, 2); console.dir({a:{b:{c:{d:1}}}}, {depth:1}); console.groupEnd(); console.dir(1)');
run('console.group("g"); console.group("h"); console.dir({a:[1,2,3]}); console.dir({x:{y:{z:{w:1}}}}, {depth:null}); console.dir("l1\\nl2"); console.dirxml("l1\\nl2")');
run('console.groupCollapsed("g"); console.dir({a:1}); console.groupEnd(); console.dir(2)');
run("const o = {a:1}; o.self = o; console.dir(o); console.dir(o, {depth:0}); console.dir(o, {depth:null}); console.dirxml(o)");
run("console.dir({a:{b:{c:{}}}}, {depth:2}); console.dir({a:{b:{c:{}}}}, {depth:3}); console.dir({a:[[[[]]]]}, {depth:2})");
run("console.dir(new Array(200).fill(1)); console.dir(new Array(200).fill(1), {depth:0}); console.dir('x'.repeat(200))");
run("console.log(1); console.dir(2); console.error(3); console.dirxml(4); console.warn(5); console.info(6); console.dir(7)");
run("console.dir(1); console.error('e'); console.dir(2)");
run("console.dir.call(console, {a:1}); const d = console.dir; d({a:1}, {depth:0}); const x = console.dirxml; x({a:2})");
run("console.dir.call(null, 1)");
run("console.dir.call(undefined, 1)");
run("console.dir.call({}, 1)");
run("const o = {dir: console.dir}; o.dir(3)");
run("console.dir(console.dir)");
run("console.dir(console.dir, {depth:0})");
run("console.dir(console.dirxml)");

// `console.table`: array de objetos com chaves diferentes, arrays de arrays, objeto de objetos, Map, Set, primitivos,
// o segundo argumento (colunas inexistentes, repetidas, não array), largura (CJK, emoji, combinantes), células aninhadas,
// array esparso e chamada sem argumentos.
for (const t of [
  "[{a:1,b:2},{b:3,c:4},{d:5}]", "[{a:1},{a:'x'},{a:null},{a:undefined},{a:true}]", "[[1,2],[3],[4,5,6]]", "[]", "{}",
  "{r:{x:1,y:2},s:{y:3,z:4}}", "{a:1,b:'s',c:[1,2]}", "new Map([[1,{a:1}],['k','v'],[{o:1},[1,2]]])", "new Map()", "new Set([1,'a',{x:1}])",
  "new Set()", "[1,'a',null,undefined,Symbol('s'),1n,-0]", "[,1,,2]", "[{a:{b:{c:{d:{e:{f:1}}}}}}]", "[{a:[1,2,3],b:{x:1},c:()=>1,d:new Date(0),e:/x/g}]",
  "[{a:'漢字',b:'😀'},{a:1}]", "[{a:'e\\u0301',b:'a\\u200bb',c:'\\u{1F468}\\u200D\\u{1F469}',d:'\\uff71'}]", "[{a:'x'.repeat(60)}]",
  "[{'名前':1,'ключ':2}]", "{'漢字':{a:1}}", "[{a:'l1\\nl2'}]", "[{a:'\\x1b[31mx\\x1b[0m'}]", "new Array(3)", "{1:{a:1},0:{a:2},b:{a:3}}",
  "'str'", "5", "null", "undefined", "Symbol('s')", "[[]]", "[{}]", "[1,[2],{a:3}]", "new Uint8Array([1,2])", "'ab'.split('')",
  "Object.create({inherited:1},{own:{value:{p:1},enumerable:true}})", "[new String('s'),new Number(1)]", "[{get a(){return 1}}]",
]) {
  run(`console.table(${t})`);
  for (const p of ["['a']", "['a','a']", "['zz']", "['a','b','a']", "[]", "[0,1]", "[1]", "['Values']", "['Key']", "[' ']", "[null,undefined]", "['漢字']"]) {
    run(`console.table(${t}, ${p})`);
  }
}
for (const p of ["'a'", "5", "{}", "null", "true", "undefined", "new Set(['a'])", "{length:1,0:'a'}", "new Proxy([], {})", "Object.assign([], {x:1})", "[Symbol('s')]", "[{toString(){return 'a'}}]", "[{toString(){throw new Error('ts')}}]"]) {
  run(`console.table([{a:1}], ${p})`);
  run(`console.table(1, ${p})`);
  run(`console.table('s', ${p})`);
}
run("console.table()");
run("console.table(undefined, ['a'])");
run("console.table([{a:1}], undefined)");
run("console.table([{a:1}], [], 'extra')");
run("console.table('s', undefined, 'extra')");
run("console.table([{get a(){ throw new Error('ga') }}])");
run("console.table({get a(){ throw new Error('ga') }})");
run("console.table([{a:1}], [{toString(){ throw new Error('cn') }}])");
run("console.table({[Symbol.iterator]: 1})");
run("console.table({[Symbol.iterator]: function*(){ yield {a:1}; yield 2; throw new Error('it') }})");
run("console.table({[Symbol.iterator]: function*(){ yield {a:1}; yield [2] }})");
run("console.table(new Proxy([{a:1}], {}))");
run("console.table(new Proxy({a:{b:1}}, { ownKeys(){ throw new Error('ok') } }))");
run("const o = {a:1}; o.self = o; console.table([o])");
run("console.table([{a:1}]); console.table([{b:2}]); console.log('x'); console.error('y'); console.table([{c:3}])");
run("console.group('g'); console.table([{a:{b:1}}]); console.table(5); console.groupEnd(); console.table([{a:1}])");
run("console.table.call(null, [{a:1}])");
run("const t = console.table; t({a:{b:1}})");
run("console.table(new Map([[1,2]]), ['a'])");
run("console.table(new Map([['k',{a:1}]]), ['a'])");
run("console.table(new Map([[1,2]]).entries())");
run("console.table(new Set([1]).values())");
run("console.table(new Map([[1,{a:1}]]).keys())");
run("console.table([{a:1},{a:2}].values())");
run("console.table('ab'.matchAll(/./g))");
run("class M extends Map {}; console.table(new M([[1,2]]))");
run("console.table(new WeakMap())");
run("console.table(function f(){})");
run("console.table(Object.assign(function f(){}, {a:{b:1}}))");
run("console.table(Object.assign(()=>1, {a:1}))");

// `console.trace`: o texto (com formato) e os frames em `    at nome (arquivo:linha:coluna)`.
run("console.trace()");
run("console.trace('hi')");
run("console.trace('hi', 1, 2)");
run("console.trace('%s|%d', 'x', 5, 'rest')");
run("console.trace('')");
run("console.trace(1)");
run("console.trace(-0)");
run("console.trace(null, undefined)");
run("console.trace({a:1})");
run("console.trace({a:{b:{c:{d:1}}}}, [1,2,3])");
run("console.trace('a\\nb')");
run("console.trace(Symbol('s'))");
run("console.trace('[%s]', Symbol('s'))");
run("console.trace('%j', 1n)");
run("console.trace(1n, true)");
run("console.trace(new Error('e'))");
run("function f() { console.trace('in f') }\nf()");
run("function f() { console.trace() }\nfunction g() { f() }\nfunction h() { g() }\nh()");
run("const a = () => console.trace('arrow');\na()");
run("const o = { m() { console.trace('m') } };\no.m()");
run("class K { constructor() { console.trace('ctor') } static s() { console.trace('static') } get p() { console.trace('getter'); return 1 } m() { console.trace('method') } }\nnew K().m();\nK.s();\nnew K().p");
run("[1, 2].forEach(function cb(x) { console.trace('cb', x) })");
run("[1].map((x) => { console.trace('map'); return x })");
run("function f() { console.trace('in eval') }\neval('f()')");
run("eval('console.trace(\"top eval\")')");
run("new Function('console.trace(\"nf\")')()");
run("function rec(n) { if (n === 0) console.trace('deep'); else rec(n - 1) }\nrec(15)");
run("function rec(n) { if (n === 0) console.trace('deep'); else rec(n - 1) }\nrec(9)");
run("function rec(n) { if (n === 0) console.trace('deep'); else rec(n - 1) }\nrec(10)");
run("Error.stackTraceLimit = 2;\nfunction rec(n) { if (n === 0) console.trace('lim'); else rec(n - 1) }\nrec(5)");
run("Error.stackTraceLimit = 0;\nfunction f() { console.trace('zero') }\nf()");
run("Error.stackTraceLimit = 100;\nfunction rec(n) { if (n === 0) console.trace('big'); else rec(n - 1) }\nrec(20)");
run("delete Error.stackTraceLimit;\nfunction f() { console.trace('nolimit') }\nf()");
run("function f() { console.trace('anon') }\n(function () { f() })()");
run("function f() { console.trace('x') }\nPromise.resolve().then(function th() { f() });\nconsole.log('sync')");
run("async function af() { console.trace('async') }\naf()");
run("async function af() { await 1; console.trace('after await') }\naf()");
run("function* g() { console.trace('gen'); yield 1 }\nfor (const v of g());");
run("function f() { console.trace('call') }\nf.call(null);\nf.apply(null, []);\nReflect.apply(f, null, [])");
run("function f() { console.trace('bound') }\nf.bind(null)()");
run("function f() { console.trace('sort') }\n[2, 1].sort(function cmp() { f(); return 0 })");
run("JSON.parse('[1]', function rev() { console.trace('reviver'); return 1 })");
run("console.group('g');\nconsole.trace('grouped');\nconsole.groupEnd();\nconsole.trace('after')");
run("console.group('g');\nfunction f() { console.trace('grouped f', 1) }\nf();\nconsole.group('h');\nf()");
run("console.log('before'); console.error('err'); console.trace('mid'); console.warn('warn'); console.log('after')");
run("console.trace.call(console, 'called');\nconst t = console.trace;\nt('detached');\nt.call(null, 'null this')");
run("function f() { console.trace('t', { a: 1 }, [2], 'x') }\nf()");
run("function f(n) { console.trace('n=' + n) }\nfor (let i = 0; i < 2; i++) f(i)");
run("const o = { get g() { console.trace('getter') } };\no.g");
run("const o = { set s(v) { console.trace('setter') } };\no.s = 1");
run("class A { static { console.trace('static block') } }");
run("class A { x = console.trace('field') }\nnew A()");
run("function F() { console.trace('newcall') }\nnew F()");
run("function f() { console.trace('tag') }\nf`x`");
run("function f() { console.trace('spread') }\nf(...[1])");
run("const p = new Proxy({}, { get(t, k) { console.trace('proxy get'); return 1 } });\np.x");
run("function f() { try { throw 1 } catch (e) { console.trace('catch') } finally { console.trace('finally') } }\nf()");
run("label: { console.trace('label') }");
run("with ({ a: 1 }) { console.trace('with') }");
run("function f() { 'use strict'; console.trace('strict') }\nf()");
run("function f() { console.trace('tail') }\nfunction g() { 'use strict'; return f() }\ng()");

// O tempo do `process` varia, os caminhos do bun viram `console_dir_case.js`.
for (const code of programs) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cd-"));
  const file = path.join(dir, "case.js");
  const tail = `\nprocess.stderr.write("\\u0000R" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`;
  fs.writeFileSync(file, code + tail);
  let result = spawnSync(process.execPath, [file], { timeout: 15000, input: "" });
  // `{depth: <BigInt>}` não é determinístico no bun 1.4.2: `ConsoleObject.rs` chama `to_int32()` no BigInt, que cai em
  // `JSC__JSValue__toInt32` = `JSValue::asInt32()`, ou seja, os 32 bits baixos do ENDEREÇO da célula do BigInt (ASLR).
  // O depth vira `max(0, ptr as i32) as u16`: com o bit 31 ligado (cerca de metade das execuções) é 0; senão são os 16
  // bits baixos do ponteiro, quase sempre bem acima da profundidade dos casos (falha só se < 5, ~0,1%). O porte escolhe
  // o comportamento "profundidade efetivamente ilimitada" (o que o bun produz na outra metade das vezes) e o golden o
  // fixa: repete o bun até a saída igualar a de `depth:null`.
  if (/\{depth:-?\d+n\}/.test(code)) {
    const reference = (() => {
      const refFile = path.join(dir, "ref.js");
      fs.writeFileSync(refFile, code.replace(/depth:-?\d+n/, "depth:null") + tail);
      return spawnSync(process.execPath, [refFile], { timeout: 15000, input: "" }).stdout;
    })();
    for (let attempt = 0; attempt < 200 && !result.stdout.equals(reference); attempt++) result = spawnSync(process.execPath, [file], { timeout: 15000, input: "" });
    if (!result.stdout.equals(reference)) throw new Error("depth BigInt nunca saiu ilimitado em: " + code);
  }
  fs.rmSync(dir, { recursive: true, force: true });
  if (result.status !== 0 && result.status !== 1) throw new Error("bun falhou em: " + code + "\n" + result.stderr);
  const at = result.stderr.lastIndexOf(Buffer.from("\u0000R"));
  if (at < 0) throw new Error("sem marcador em: " + code + "\n" + result.stderr);
  const normalize = (buffer) => Buffer.from(buffer.toString("latin1").split(file).join("console_dir_case.js"), "latin1");
  emitRow(
    [
      JSON.stringify(code),
      JSON.stringify(normalize(result.stdout).toString("hex")),
      JSON.stringify(normalize(result.stderr.subarray(0, at)).toString("hex")),
      result.stderr.subarray(at + 2).toString("utf8"),
    ].join("\t"),
  );
}
