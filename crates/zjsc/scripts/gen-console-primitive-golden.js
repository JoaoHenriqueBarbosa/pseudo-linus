// Gera tests/golden/console_primitive_bun.tsv: `console.log/info/debug` (stdout) e `console.error/warn` (stderr) com
// argumentos primitivos e as substituições de formato `%s %d %i %f %o %O %j %c %%`, medidos no bun 1.4.2.
// Colunas (todas em JSON): a fonte do programa, os bytes do stdout em hex, os bytes do stderr em hex e o valor da
// variável global `R` (a exceção, como `Nome|mensagem`, ou `<undefined>`). Objetos ficam para outra fatia.
// Também `%d` de subnormais, `count`/`countReset`, `time`/`timeLog`/`timeEnd` (o `[tempo]` no começo da linha do stderr
// vira `[T]`, o teste em Rust normaliza igual), `assert` e `group`/`groupCollapsed`/`groupEnd`.
// Uso: bun scripts/gen-console-primitive-golden.js > tests/golden/console_primitive_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const { emitRow } = require("./golden-prelude.js");

const E = "var E = function (e) { return e.name + '|' + e.message };\n";
const programs = [];
const run = (code) => programs.push(E + `try { ${code} } catch (e) { R = E(e) }`);

const primitives = [
  '"a"', '""', '"a b"', "1", "0", "-0", "1.5", "-1.5", "NaN", "Infinity", "-Infinity", "1e21", "1e-7", "123456789012345680000",
  "0.1+0.2", "1n", "-1n", "0n", "2n**70n", "true", "false", "null", "undefined", 'Symbol("s")', "Symbol()", "Symbol.iterator", '"\\u00e9"',
  '"\\ud800"', '"x\\ny"', '"\\u0000"',
];
for (const method of ["log", "info", "debug", "error", "warn"]) {
  run(`console.${method}()`);
  for (const p of primitives) run(`console.${method}(${p})`);
  run(`console.${method}("a", 1, -0, NaN, 1n, true, null, undefined, Symbol("s"))`);
  run(`console.${method}(1, "a", 2)`);
  run(`console.${method}("%s", "x")`);
  run(`console.${method}("%d %i", "4", 5)`);
}

// Substituições de formato.
const specs = ["s", "d", "i", "f", "o", "O", "j", "c"];
const args = [
  '"a"', '""', '"42"', '"42.9"', '"1.5x"', '" 12 "', '"0x10"', '"1e3"', "0", "-0", "1", "1.5", "-1.5", "NaN", "Infinity", "1e21", "1e-7",
  "1n", "-1n", "2n**70n", "true", "false", "null", "undefined", 'Symbol("s")', '"a\\"b"', '"\\u00e9"',
];
for (const spec of specs) {
  for (const a of args) {
    run(`console.log("%${spec}", ${a})`);
    run(`console.log("[%${spec}]", ${a}, "rest", 7)`);
  }
  run(`console.log("%${spec}")`);
  run(`console.log("%${spec}%${spec}", 1)`);
  run(`console.log("a%${spec}b%${spec}c", "x", 2)`);
  run(`console.log(1, "%${spec}", 2)`);
  run(`console.log("%${spec}", "a", "b")`);
}
for (const f of ["%%", "%", "100%", "%%s", "%%%s", "%%%%", "%x", "%5d", "%-s", "%s%", "% s", "%\n", "a%", "%%d", "%s%%", "%c%c", "%c%s", "%s%c", "%s%s%s", "%i%i", "%d%f", "%o%O%j"]) {
  run(`console.log(${JSON.stringify(f)})`);
  run(`console.log(${JSON.stringify(f)}, 1)`);
  run(`console.log(${JSON.stringify(f)}, 1, 2, 3)`);
  run(`console.log(${JSON.stringify(f)}, "a", "b")`);
}
for (const f of ["%x%s", "%5%s", "%-%s", "%%x%s", "%y%%", "%%%s %s", "%%%d", "%%a%s", "%s%%%s", "%%%%%s", "%%%c", "%%%", "a%%b%sc", "%%s%s", "%s%%s%s", "%%%%%%", "%%%%%%%%", "%s %", "%s%%s", "%x%%", "%%%x%s"]) {
  run(`console.log(${JSON.stringify(f)}, 1, 2)`);
  run(`console.log(${JSON.stringify(f)}, 1)`);
}
for (const [label, value] of [["1e21", "1e21"], ["1e20", "1e20"], ["str1e21", '"1e21"'], ["1e300", "1e300"], ["123456789012", "123456789012"], ["2p53", "2**53"], ["2p31", "2**31"], ["-2p31-1", "-(2**31)-1"], ["1.9e10", "1.9e10"], ["1e-7", "1e-7"], ["str1e-7", '"1e-7"'], ['".5"', '".5"'], ['"-.5"', '"-.5"'], ['"+5"', '"+5"'], ['"Infinity"', '"Infinity"'], ["-Infinity", "-Infinity"], ["[5]", "[5]"], ["{}", "{}"], ['"12abc"', '"12abc"'], ['"0b11"', '"0b11"'], ['"0o7"', '"0o7"'], ["0.5", "0.5"], ["-0.5", "-0.5"], ["1e-6", "1e-6"], ["123e-20", "123e-20"], ["1.5e300", "1.5e300"], ["123456789.9", "123456789.9"], ["9.99e22", "9.99e22"], ["2.5e-5", "2.5e-5"], ['"0x1f"', '"0x1f"'], ['"-0"', '"-0"'], ['"  "', '"  "'], ["[]", "[]"], ["-1e20", "-1e20"], ["-1e21", "-1e21"], ["1e19", "1e19"], ["9223372036854775807", "9223372036854775807"], ["-9223372036854775808", "-9223372036854775808"]]) {
  run(`console.log("%d|%i|%f", ${value}, ${value}, ${value})`);
}
run('console.log("%c", "color:red")');
run('console.log("x%cy", "color:red", "z")');
run('console.log("%c%c", "a", "b", "c")');
run('console.log(5, "%s", "x")');
run('console.log(Symbol("a"), "%s", "x")');
run('console.log("%s", Symbol("a"), 1)');
run('console.log("%s", "a", Symbol("a"))');
run('console.log("%d", Symbol("a"))');
run('console.log("%i", Symbol("a"))');
run('console.log("%f", Symbol("a"))');
run('console.log("%j", Symbol("a"))');
run('console.log("%j", 1n)');
run('console.log("%j", undefined)');
run('console.log("%j", "a")');
run('console.log("[%s][%j]", "a", 1n, "c")');
run('console.log("%s|%s|%s|%s|%s|%s|%s", "a", 1, -0, 1n, null, undefined, Symbol("s"))');
run('console.log("%d|%d|%d|%d|%d|%d|%d", "42", 1.5, -0, 1n, "x", null, Symbol("s"))');
run('console.log("%o|%O|%j", "s", "t", "u")');
run('console.log("%s", { toString() { return "obj" } })');
run('console.log("%d", { valueOf() { return 7 } })');
run('console.log("a%s", 1, "b%s", 2)');
run('console.log("a", "b%s", 1)');
run('console.log("%s", "%s", "%s")');
run('console.log(1, "%d%%", 2)');
run('console.log("x", "y%%", "z")');
run('console.log("%s", "a", "%s", "b", "%s")');
run('console.log("%s", Symbol("a"), "%s", 1)');
run('console.log("a", "%s", Symbol("a"))');
run('console.log("a", "%s", 1n, "%d", 2n)');
run('console.error("")');
run('console.warn("")');
run('console.log("%d", new Number(5))');
run('console.log("%s", function f() {})');
run('console.log("%s", class A {})');
run('console.log("%d", "")');
run('console.log("%d", " ")');
run('console.log("%d", [])');
run('console.log("%s", [1, 2])');
run('console.log("%s", "a\\u0000b")');

// `%d`/`%i` de subnormais e de valores minúsculos (abaixo de 1e-6 o bun multiplica por 10 até chegar a 1).
for (const v of ["5e-324", "-5e-324", "1e-323", "1.5e-323", "2.2250738585072014e-308", "2.225073858507201e-308", "1e-300", "1e-100", "1e-30", "1e-14", "1e-13", "1e-12", "1e-11", "1e-10", "1e-9", "1e-8", "1e-7", "9.999999999999999e-7", "1e-6", "9.99e-7", "5e-7", "6.999999999999999e-7", "8.999999999999999e-8", "3.7e-50", "9.9e-200", "-1e-11", "-1e-7", "-9.9e-9", "2**-1074", "2**-1022", "2**-1000", "Number.EPSILON", "0.1**20", "1.23e-18"]) {
  run(`console.log("%d|%i|%f", ${v}, ${v}, ${v})`);
}

// count, countReset.
for (const code of [
  "console.count()", "console.count(); console.count()", 'console.count("a"); console.count("a"); console.count("b")',
  "console.count(undefined); console.count(null); console.count(1); console.count(-0); console.count(1n); console.count(true); console.count(NaN)",
  'console.count(""); console.count("a\\nb"); console.count("A"); console.count("a")',
  'console.count("x"); console.countReset("x"); console.count("x")', "console.countReset(); console.count(); console.countReset(); console.count()",
  'console.countReset("zz"); console.countReset("zz"); console.count("zz")', "console.count(undefined); console.countReset(); console.count()",
  'console.count(Symbol("q"))', 'console.countReset(Symbol("q"))', 'console.count("a", "b"); console.count("a", "c")', 'console.count("a"); console.group("g"); console.count("a"); console.groupEnd()',
]) run(code);

// time, timeLog, timeEnd (o número de `[tempo]` é normalizado em `normalizeTime`).
for (const code of [
  "console.time(); console.timeLog(); console.timeEnd(); console.timeEnd(); console.timeLog()",
  'console.time("t"); console.timeLog("t"); console.timeLog("t", "a", 1, -0, 1n, null, undefined, true, Symbol("s")); console.timeEnd("t"); console.timeEnd("t"); console.timeLog("t")',
  'console.time("t"); console.timeLog("t", "%s", "fmt"); console.timeLog("t", "a %s", "b"); console.timeLog("t", "%d", 5, 6); console.timeLog("t", 1, "%s", 2)',
  'console.time("t"); console.timeLog("t", ""); console.timeLog("t", "", ""); console.timeLog("t", "x\\ny"); console.timeLog("t", undefined)',
  'console.time("t"); console.time("t"); console.timeEnd("t"); console.timeEnd("t")',
  'console.timeEnd("nope"); console.timeLog("nope"); console.timeLog("nope", 1)',
  'console.time(1); console.timeEnd(1); console.time(null); console.timeEnd(null); console.time(undefined); console.timeEnd(); console.time(-0); console.timeEnd(0)',
  'console.time(""); console.timeLog("", "x"); console.timeLog("", "x", "y"); console.timeLog(""); console.timeLog("", 1); console.timeEnd("")',
  'console.time("a"); console.timeEnd("a", "extra"); console.timeEnd("a")', 'console.time("a"); console.time("b"); console.timeEnd("b"); console.timeEnd("a")',
  'console.time(Symbol("q"))', 'console.timeEnd(Symbol("q"))', 'console.timeLog(Symbol("q"))',
  'console.group("g"); console.time("t"); console.timeLog("t", "x"); console.timeEnd("t"); console.groupEnd()',
]) run(code);

// assert com primitivos.
for (const cond of ["false", "0", "-0", "0n", '""', "null", "undefined", "NaN", "true", "1", '"0"', '"a"', "1n", "Symbol()"]) {
  run(`console.assert(${cond})`);
  run(`console.assert(${cond}, "m")`);
}
for (const code of [
  "console.assert()", 'console.assert(false, "a", 1, null)', 'console.assert(false, "%s", "x")', 'console.assert(false, "%s %d", "x", 4)', 'console.assert(false, "a", "%s", "b")',
  "console.assert(false, undefined)", 'console.assert(false, "")', 'console.assert(false, "", 1)', "console.assert(false, 1)", "console.assert(false, 1n, -0, true)",
  'console.assert(false, Symbol("s"))', 'console.assert(false, "%s", Symbol("s"))', 'console.assert(false, "[%s]", Symbol("s"))', 'console.assert(false, "%j", 1n)',
  'console.assert(false, "a\\nb")', 'console.assert(true, "no"); console.assert(false, "yes")', "console.assert(false); console.assert(false)",
]) run(code);

// group, groupCollapsed, groupEnd (recuo de dois espaços por nível, só na primeira linha de cada chamada).
for (const code of [
  'console.group(); console.log("a"); console.groupEnd(); console.log("b")', 'console.group("g"); console.log("a"); console.error("e"); console.warn("w"); console.info("i"); console.debug("d"); console.groupEnd(); console.log("z")',
  'console.group("a", "b"); console.group(); console.group(1, 2); console.log(3)', 'console.group("%s", "fmt", "z"); console.log("x")',
  'console.group("g"); console.log(); console.log(""); console.error(); console.error(""); console.warn(); console.groupEnd()',
  'console.group("g"); console.log("l1\\nl2\\n\\nl4"); console.log("\\nlead"); console.log("x", "y\\nz"); console.log("%s", "p\\nq")',
  'console.group("a\\nb"); console.log("x")', 'console.group(null); console.group(undefined); console.group(""); console.group(true); console.group(-0); console.group(1n); console.log("x")',
  'console.group(Symbol("s")); console.log("x")', 'try { console.group("%s", Symbol("s")) } catch (e) { console.log("c") } console.log("x")',
  'console.group("a"); try { console.log("[%s]", Symbol("s")) } catch (e) { console.log("c") } console.log("x")',
  'console.groupEnd(); console.groupEnd(); console.group("a"); console.log("x"); console.groupEnd(); console.log("y")',
  'console.group(); console.group(); console.log(1); console.groupEnd(); console.log(2); console.groupEnd(); console.log(3); console.groupEnd("ignored"); console.log(4)',
  'console.groupCollapsed("gc"); console.log("in"); console.groupEnd(); console.log("after")',
  'console.group("a"); console.groupCollapsed("c"); console.log("in"); console.groupEnd(); console.log("mid"); console.groupEnd(); console.log("end")',
  'console.groupCollapsed(); console.log("n"); console.groupEnd(); console.groupCollapsed("c"); console.groupCollapsed("d"); console.log("z")',
  'console.group("a"); console.groupCollapsed(); console.log("n"); console.groupCollapsed(undefined); console.groupCollapsed(""); console.groupEnd()',
  'console.group("a"); console.groupCollapsed("%s", "f"); console.groupCollapsed(1, 2); console.log("n")',
  'console.group("a"); console.assert(false); console.assert(false, "a\\nb"); console.assert(false, "m", "n"); console.assert(true)',
  'console.group("g"); console.count("c"); console.countReset("c"); console.count("c"); console.groupEnd()',
  'console.group("a"); console.group("b"); console.log(1); console.groupEnd(); console.groupEnd(); console.groupEnd(); console.log(2)',
  'console.group("a"); console.log("%s|%d|%j", "s", 2, 3); console.log(1, 2, 3); console.log("%c", "css")',
]) run(code);

// O tempo de `timeLog`/`timeEnd` varia: `[0.01ms]`, `[1.60s]` viram `[T]` no começo da linha do stderr.
function normalizeTime(buffer) {
  return Buffer.from(buffer.toString("latin1").replace(/(^|\n)\[[0-9.]+[a-z]+\]/g, "$1[T]"), "latin1");
}

for (const code of programs) {
  const source = code.replace(/[^\x00-\x7f]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cp-"));
  const file = path.join(dir, "case.js");
  fs.writeFileSync(
    file,
    `(0, eval)("var R");\n(0, eval)(${JSON.stringify(source)});\nprocess.stderr.write("\\u0000R" + JSON.stringify(String(globalThis.R === undefined ? "<undefined>" : globalThis.R)));\n`,
  );
  // Sem `encoding`: os dois fluxos ficam em bytes; o stderr do programa vem antes do marcador `\0R`.
  const result = spawnSync(process.execPath, [file], { timeout: 15000, input: "" });
  fs.rmSync(dir, { recursive: true, force: true });
  if (result.status !== 0) throw new Error("bun falhou em: " + source + "\n" + result.stderr);
  const at = result.stderr.lastIndexOf(Buffer.from("\u0000R"));
  emitRow(
    [
      JSON.stringify(source),
      JSON.stringify(result.stdout.toString("hex")),
      JSON.stringify(normalizeTime(result.stderr.subarray(0, at)).toString("hex")),
      result.stderr.subarray(at + 2).toString("utf8"),
    ].join("\t"),
  );
}
