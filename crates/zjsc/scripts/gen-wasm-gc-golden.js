// Gera tests/golden/wasm_gc_bun.tsv: programas de WebAssembly GC e tipos de referência (struct, array, i31, ref.test,
// ref.cast, br_on_cast, conversões any/extern, subtipagem e rec groups, tabelas, return_call, exceções com
// try_table, multi-valor, traps), avaliados no bun. Os módulos saem de wat via `wasm-tools parse` e entram no fonte
// de cada programa em hexadecimal (`HX("...")`, de tests/golden/wasm_gc_bun_extra.js). Colunas: fonte, depois o JSON
// do log, ou `error<TAB>name<TAB>message JSON` se o programa lançou de forma síncrona. Cada programa roda num
// processo bun próprio, com timeout. Uso:
//   bun scripts/gen-wasm-gc-golden.js > tests/golden/wasm_gc_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const golden = path.join(__dirname, "../tests/golden");
const harness = ["wasm_js_bun_harness.js", "wasm_exceptions_bun_extra.js", "wasm_gc_bun_extra.js"]
  .map((name) => fs.readFileSync(path.join(golden, name), "utf8"))
  .join("\n");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wasm-gc-golden-"));

let moduleCount = 0;
const hexOf = (wat) => {
  const file = path.join(tmp, `m${moduleCount++}.wat`);
  fs.writeFileSync(file, wat);
  const parsed = spawnSync("wasm-tools", ["parse", file, "-o", file + ".wasm"], { encoding: "utf8" });
  if (parsed.status !== 0) throw new Error("wat inválido:\n" + parsed.stderr + "\n" + wat);
  return fs.readFileSync(file + ".wasm").toString("hex");
};

const programs = [];
const seen = new Set();
const add = (source) => {
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  if (!seen.has(source)) {
    seen.add(source);
    programs.push(source);
  }
};
// Um programa por (função, argumentos): `L(C(() => x.f(args)))`; `render` formata a chamada.
const calls = (prefix, name, argLists, wrap = (c) => `L(C(() => ${c}))`) => {
  for (const args of argLists) add(prefix + wrap(`x.${name}(${args.join(", ")})`));
};
const prefixOf = (wat, imports) => `var x = RUN(HX("${hexOf(wat)}")${imports ? ", " + imports : ""}); `;
const one = (values) => values.map((v) => [v]);

// ---------- structs com campos packed ----------
const S = prefixOf(String.raw`(module
 (type $s (sub (struct (field i8) (field (mut i16)) (field i32) (field (mut f64)))))
 (type $b (sub $s (struct (field i8) (field (mut i16)) (field i32) (field (mut f64)) (field (mut i64)))))
 (func (export "i8s") (param i32) (result i32) (struct.get_s $s 0 (struct.new $s (local.get 0) (i32.const 0) (i32.const 0) (f64.const 0))))
 (func (export "i8u") (param i32) (result i32) (struct.get_u $s 0 (struct.new $s (local.get 0) (i32.const 0) (i32.const 0) (f64.const 0))))
 (func (export "i16s") (param i32) (result i32) (struct.get_s $s 1 (struct.new $s (i32.const 0) (local.get 0) (i32.const 0) (f64.const 0))))
 (func (export "i16u") (param i32) (result i32) (struct.get_u $s 1 (struct.new $s (i32.const 0) (local.get 0) (i32.const 0) (f64.const 0))))
 (func (export "set16s") (param i32 i32) (result i32) (local $o (ref null $s))
   (local.set $o (struct.new $s (i32.const 0) (local.get 0) (i32.const 0) (f64.const 0)))
   (struct.set $s 1 (local.get $o) (local.get 1))
   (struct.get_s $s 1 (local.get $o)))
 (func (export "set16u") (param i32 i32) (result i32) (local $o (ref null $s))
   (local.set $o (struct.new $s (i32.const 0) (local.get 0) (i32.const 0) (f64.const 0)))
   (struct.set $s 1 (local.get $o) (local.get 1))
   (struct.get_u $s 1 (local.get $o)))
 (func (export "i32f") (param i32) (result i32) (struct.get $s 2 (struct.new $s (i32.const 0) (i32.const 0) (local.get 0) (f64.const 0))))
 (func (export "f64f") (param f64) (result f64) (struct.get $s 3 (struct.new $s (i32.const 0) (i32.const 0) (i32.const 0) (local.get 0))))
 (func (export "i64f") (param i64) (result i64) (struct.get $b 4 (struct.new $b (i32.const 1) (i32.const 2) (i32.const 3) (f64.const 4.5) (local.get 0))))
 (func (export "supget") (result i32) (struct.get $s 2 (struct.new $b (i32.const 1) (i32.const 2) (i32.const 77) (f64.const 0) (i64.const 0))))
 (func (export "nullget") (result i32) (struct.get $s 2 (ref.null $s)))
 (func (export "nullgets") (result i32) (struct.get_s $s 0 (ref.null $s)))
 (func (export "nullset") (struct.set $s 1 (ref.null $s) (i32.const 1)))
 (func (export "dflt64") (result i64) (struct.get $b 4 (struct.new_default $b)))
 (func (export "dfltf") (result f64) (struct.get $b 3 (struct.new_default $b)))
 (func (export "dflt8") (result i32) (struct.get_u $b 0 (struct.new_default $b)))
 (func (export "mk") (result (ref $s)) (struct.new $s (i32.const 1) (i32.const 2) (i32.const 3) (f64.const 4)))
 (func (export "mkb") (result (ref $b)) (struct.new_default $b))
 (func (export "mkn") (result (ref null $s)) (ref.null $s))
 (func (export "geti32") (param (ref $s)) (result i32) (struct.get $s 2 (local.get 0)))
 (func (export "geti32n") (param (ref null $s)) (result i32) (struct.get $s 2 (local.get 0)))
 (func (export "getb") (param (ref $b)) (result i64) (struct.get $b 4 (local.get 0)))
)`);
calls(S, "i8s", one([0, 1, 127, 128, 255, 256, 257, -1, -128, -129, 383, 2147483647, -2147483648, 4294967295, 1.5, "'7'", "'x'", "null"]));
calls(S, "i8u", one([0, 1, 127, 128, 255, 256, -1, -128, 511, 4294967295]));
calls(S, "i16s", one([0, 1, 32767, 32768, 65535, 65536, 65537, -1, -32768, -32769, 4294967295, 2147483648]));
calls(S, "i16u", one([0, 32767, 32768, 65535, 65536, -1, -32768, -32769, 1e10]));
calls(S, "set16s", [[0, 0], [1, 40000], [1, 32768], [1, -1], [5, 65535], [5, 65536], [0, 98304]]);
calls(S, "set16u", [[0, 0], [1, 40000], [1, -40000], [5, 65535], [5, 131071]]);
calls(S, "i32f", one([0, 1, -1, 2147483647, 2147483648, 4294967296, 1.9, "'12'", "undefined"]));
calls(S, "f64f", one([0, -0, 1.5, "NaN", "Infinity", "'1e3'", "null"]));
calls(S, "i64f", one(["0n", "1n", "-1n", "2n ** 63n - 1n", "2n ** 64n", "5", "'9'", "true"]));
for (const f of ["supget", "nullget", "nullgets", "nullset", "dflt64", "dfltf", "dflt8"]) calls(S, f, [[]]);
add(S + "var o = x.mk(); L(typeof o); L(Object.getPrototypeOf(o)); L(Object.isFrozen(o)); L(Object.isExtensible(o)); L(Object.keys(o).length)");
add(S + "var o = x.mk(); L(String(Object.prototype.toString.call(o))); L(T(() => String(o))); L(T(() => o + 1)); L(T(() => JSON.stringify(o)))");
add(S + "var o = x.mk(); L(T(() => { o.a = 1; return Object.keys(o).length })); L(T(() => { 'use strict'; o.a = 1; return 1 })); L(T(() => { delete o.a; return 1 }))");
add(S + "var o = x.mk(); L(x.geti32(o)); L(x.geti32n(o)); L(C(() => x.geti32n(null))); L(C(() => x.geti32(null)))");
add(S + "L(C(() => x.geti32({}))); L(C(() => x.geti32(1))); L(C(() => x.geti32('s'))); L(C(() => x.geti32(undefined)))");
add(S + "var b = x.mkb(); L(x.geti32(b)); L(String(x.getb(b))); L(C(() => x.getb(x.mk())))");
add(S + "L(x.mkn()); L(x.mkn() === null); L(C(() => x.mkn()))");
add(S + "var a = x.mk(), b = x.mk(); L(a === b); L(a === a); L(Object.is(a, a)); L([a].includes(a)); L(new Set([a, b]).size)");
add(S + "var o = x.mk(); var m = new Map([[o, 1]]); L(m.get(o)); L(m.get(x.mk())); var w = new WeakMap(); L(T(() => w.set(o, 1) === w))");
add(S + "L(T(() => x.mk().foo)); L(T(() => x.mk()[0])); L(T(() => Object.getOwnPropertyNames(x.mk()).length)); L(T(() => 'a' in x.mk()))");
add(S + "L(T(() => Object.setPrototypeOf(x.mk(), {}))); L(T(() => Object.defineProperty(x.mk(), 'a', { value: 1 }))); L(T(() => Object.freeze(x.mk()) !== null))");
add(S + "L(T(() => x.mk() == x.mk())); L(T(() => x.mk() == 1)); L(T(() => typeof x.mk().valueOf))");

// ---------- arrays ----------
const A = prefixOf(String.raw`(module
 (type $a8 (array (mut i8)))
 (type $a16 (array (mut i16)))
 (type $a32 (array (mut i32)))
 (type $ap (array i16))
 (type $af (array (mut funcref)))
 (type $ar (array (mut anyref)))
 (type $ft (func (result i32)))
 (data $d "\01\02\03\04\05\06\07\08")
 (elem $e func $f $g)
 (func $f (result i32) i32.const 11)
 (func $g (result i32) i32.const 22)
 (func (export "len") (param i32 i32) (result i32) (array.len (array.new $a32 (local.get 1) (local.get 0))))
 (func (export "get") (param i32 i32) (result i32) (array.get $a32 (array.new $a32 (i32.const 7) (local.get 0)) (local.get 1)))
 (func (export "gets8") (param i32) (result i32) (array.get_s $a8 (array.new_fixed $a8 3 (i32.const 200) (i32.const 127) (i32.const 128)) (local.get 0)))
 (func (export "getu8") (param i32) (result i32) (array.get_u $a8 (array.new_fixed $a8 3 (i32.const 200) (i32.const 127) (i32.const 128)) (local.get 0)))
 (func (export "gets16") (param i32) (result i32) (array.get_s $a16 (array.new_fixed $a16 3 (i32.const 40000) (i32.const 32767) (i32.const -1)) (local.get 0)))
 (func (export "getu16") (param i32) (result i32) (array.get_u $a16 (array.new_fixed $a16 3 (i32.const 40000) (i32.const 32767) (i32.const -1)) (local.get 0)))
 (func (export "fixed") (param i32) (result i32) (array.get $a32 (array.new_fixed $a32 4 (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4)) (local.get 0)))
 (func (export "fixed0") (result i32) (array.len (array.new_fixed $a32 0)))
 (func (export "newdata") (param i32 i32 i32) (result i32) (array.get_u $a8 (array.new_data $a8 $d (local.get 0) (local.get 1)) (local.get 2)))
 (func (export "newdatalen") (param i32 i32) (result i32) (array.len (array.new_data $a8 $d (local.get 0) (local.get 1))))
 (func (export "newdata16") (param i32 i32 i32) (result i32) (array.get_u $a16 (array.new_data $a16 $d (local.get 0) (local.get 1)) (local.get 2)))
 (func (export "newdata32") (param i32 i32 i32) (result i32) (array.get $a32 (array.new_data $a32 $d (local.get 0) (local.get 1)) (local.get 2)))
 (func (export "newelemlen") (param i32 i32) (result i32) (array.len (array.new_elem $af $e (local.get 0) (local.get 1))))
 (func (export "newelemcall") (param i32 i32 i32) (result i32)
   (call_ref $ft (ref.cast (ref null $ft) (array.get $af (array.new_elem $af $e (local.get 0) (local.get 1)) (local.get 2)))))
 (func (export "copy") (param i32 i32 i32) (result i32) (local $a (ref null $a32))
   (local.set $a (array.new_fixed $a32 5 (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4) (i32.const 5)))
   (array.copy $a32 $a32 (local.get $a) (local.get 0) (local.get $a) (local.get 1) (local.get 2))
   (i32.add (i32.add (i32.add (array.get $a32 (local.get $a) (i32.const 0)) (i32.mul (array.get $a32 (local.get $a) (i32.const 1)) (i32.const 10)))
     (i32.add (i32.mul (array.get $a32 (local.get $a) (i32.const 2)) (i32.const 100)) (i32.mul (array.get $a32 (local.get $a) (i32.const 3)) (i32.const 1000))))
     (i32.mul (array.get $a32 (local.get $a) (i32.const 4)) (i32.const 10000))))
 (func (export "fill") (param i32 i32 i32) (result i32) (local $a (ref null $a32))
   (local.set $a (array.new_fixed $a32 5 (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4) (i32.const 5)))
   (array.fill $a32 (local.get $a) (local.get 0) (local.get 1) (local.get 2))
   (i32.add (i32.add (i32.add (array.get $a32 (local.get $a) (i32.const 0)) (i32.mul (array.get $a32 (local.get $a) (i32.const 1)) (i32.const 10)))
     (i32.add (i32.mul (array.get $a32 (local.get $a) (i32.const 2)) (i32.const 100)) (i32.mul (array.get $a32 (local.get $a) (i32.const 3)) (i32.const 1000))))
     (i32.mul (array.get $a32 (local.get $a) (i32.const 4)) (i32.const 10000))))
 (func (export "fill8") (param i32) (result i32) (local $a (ref null $a8))
   (local.set $a (array.new_default $a8 (i32.const 2)))
   (array.fill $a8 (local.get $a) (i32.const 0) (local.get 0) (i32.const 2))
   (array.get_s $a8 (local.get $a) (i32.const 1)))
 (func (export "set") (param i32 i32 i32) (result i32) (local $a (ref null $a32))
   (local.set $a (array.new_default $a32 (local.get 0)))
   (array.set $a32 (local.get $a) (local.get 1) (local.get 2))
   (array.get $a32 (local.get $a) (local.get 1)))
 (func (export "set8") (param i32 i32) (result i32) (local $a (ref null $a8))
   (local.set $a (array.new_default $a8 (local.get 0)))
   (array.set $a8 (local.get $a) (i32.const 0) (local.get 1))
   (array.get_s $a8 (local.get $a) (i32.const 0)))
 (func (export "initdata") (param i32 i32 i32) (result i32) (local $a (ref null $a8))
   (local.set $a (array.new_default $a8 (i32.const 4)))
   (array.init_data $a8 $d (local.get $a) (local.get 0) (local.get 1) (local.get 2))
   (i32.add (i32.mul (array.get_u $a8 (local.get $a) (i32.const 0)) (i32.const 100)) (array.get_u $a8 (local.get $a) (i32.const 3))))
 (func (export "dflt") (param i32) (result i32) (array.get $a32 (array.new_default $a32 (local.get 0)) (i32.const 0)))
 (func (export "refnull") (param i32) (result i32) (ref.is_null (array.get $ar (array.new_default $ar (local.get 0)) (i32.const 0))))
 (func (export "reflen") (param i32) (result i32) (array.len (array.new_default $ar (local.get 0))))
 (func (export "mk") (param i32) (result (ref $a32)) (array.new_default $a32 (local.get 0)))
 (func (export "mkp") (result (ref $ap)) (array.new_fixed $ap 2 (i32.const 1) (i32.const 2)))
 (func (export "arrlen") (param (ref $a32)) (result i32) (array.len (local.get 0)))
 (func (export "arrlenn") (param (ref null $a32)) (result i32) (array.len (local.get 0)))
 (func (export "nullget") (result i32) (array.get $a32 (ref.null $a32) (i32.const 0)))
 (func (export "nulllen") (result i32) (array.len (ref.null $a32)))
 (func (export "nullset") (array.set $a32 (ref.null $a32) (i32.const 0) (i32.const 1)))
)`);
calls(A, "len", [[0, 0], [3, 1], [3, 2], [100, 9], [65536, 0], [-1, 0], [1.5, 0]]);
calls(A, "get", [[3, 0], [3, 2], [3, 3], [3, -1], [0, 0], [1, 4294967295], [3, 2147483648], [10, "'3'"]]);
calls(A, "gets8", one([0, 1, 2, 3, -1]));
calls(A, "getu8", one([0, 1, 2, 3]));
calls(A, "gets16", one([0, 1, 2, 3]));
calls(A, "getu16", one([0, 1, 2]));
calls(A, "fixed", one([0, 1, 2, 3, 4, -1]));
calls(A, "fixed0", [[]]);
calls(A, "newdata", [[0, 8, 0], [0, 8, 7], [0, 8, 8], [2, 4, 0], [2, 4, 3], [6, 2, 1], [6, 3, 0], [8, 0, 0], [9, 0, 0], [0, 9, 0], [-1, 1, 0], [4, 0, 0]]);
calls(A, "newdatalen", [[0, 8], [3, 0], [8, 0], [8, 1], [7, 2], [0, 0], [0, 9]]);
calls(A, "newdata16", [[0, 4, 0], [0, 4, 1], [0, 4, 3], [0, 8, 3], [1, 2, 0], [7, 2, 0], [0, 3, 0]]);
calls(A, "newdata32", [[0, 2, 0], [0, 2, 1], [0, 2, 2], [4, 1, 0], [6, 1, 0]]);
calls(A, "newelemlen", [[0, 2], [0, 0], [1, 1], [2, 0], [2, 1], [0, 3], [3, 0]]);
calls(A, "newelemcall", [[0, 2, 0], [0, 2, 1], [1, 1, 0], [0, 2, 2], [0, 1, 1]]);
calls(A, "copy", [[0, 1, 2], [1, 0, 2], [0, 0, 5], [0, 2, 3], [2, 0, 3], [0, 0, 0], [5, 0, 0], [5, 0, 1], [0, 5, 1], [3, 0, 3], [0, 3, 3], [4, 0, 2], [6, 6, 0]]);
calls(A, "fill", [[0, 9, 5], [1, 9, 2], [4, 9, 1], [4, 9, 2], [5, 9, 0], [5, 9, 1], [6, 9, 0], [0, 9, 6], [2, -1, 3]]);
calls(A, "fill8", one([0, 127, 128, 255, 256, -1, -129]));
calls(A, "set", [[3, 0, 5], [3, 2, 5], [3, 3, 5], [0, 0, 1], [3, -1, 1], [3, 1, "'8'"]]);
calls(A, "set8", [[1, 100], [1, 200], [1, 300], [1, -1], [0, 1]]);
calls(A, "initdata", [[0, 0, 4], [2, 0, 4], [0, 4, 4], [4, 4, 4], [0, 7, 2], [0, 5, 4], [9, 0, 0], [0, 0, 5], [8, 4, 0]]);
calls(A, "dflt", one([0, 1, 10]));
calls(A, "refnull", one([0, 1]));
calls(A, "reflen", one([0, 1, 5]));
for (const f of ["nullget", "nulllen", "nullset"]) calls(A, f, [[]]);
add(A + "L(C(() => x.dflt(-1)))");
add(A + "L(C(() => x.len(-1, 0)))");
add(A + "L(C(() => x.reflen(2147483647)))");
add(A + "var a = x.mk(3); L(typeof a); L(Object.getPrototypeOf(a)); L(Object.isFrozen(a)); L(Object.keys(a).length); L(a.length); L(a[0]); L(T(() => a.length = 1))");
add(A + "var a = x.mk(3); L(x.arrlen(a)); L(x.arrlenn(a)); L(C(() => x.arrlenn(null))); L(C(() => x.arrlen(null))); L(C(() => x.arrlen({}))); L(C(() => x.arrlen(x.mkp())))");
add(A + "var p = x.mkp(); L(typeof p); L(Object.prototype.toString.call(p)); L(C(() => x.arrlen(p)))");
add(A + "L(x.mk(0) === x.mk(0)); L(Array.isArray(x.mk(1))); L(T(() => [...x.mk(1)])); L(T(() => Array.from(x.mk(2)).length))");

// ---------- casts, i31, any/extern ----------
const mkChain = (cases) => {
  // k (local 0) escolhe o valor; cada caso é um `(ref ...)` já com o tipo anyref
  let body = cases[cases.length - 1];
  for (let k = cases.length - 2; k >= 0; k--) body = `(if (result anyref) (i32.eq (local.get 0) (i32.const ${k})) (then ${cases[k]}) (else ${body}))`;
  return body;
};
const mkCases = [
  "(ref.null any)",
  "(ref.i31 (i32.const 5))",
  "(struct.new $s (i32.const 1))",
  "(struct.new $b (i32.const 1) (i32.const 2))",
  "(array.new $arr (i32.const 0) (i32.const 2))",
  "(any.convert_extern (ref.null extern))",
  "(ref.i31 (i32.const -1))",
  "(struct.new $o (i32.const 9))",
  "(ref.i31 (i32.const 1073741823))",
  "(ref.i31 (i32.const 1073741824))",
];
const testFns = {
  ti31: "(ref i31)", tnull_i31: "(ref null i31)", ts: "(ref $s)", tns: "(ref null $s)", tb: "(ref $b)", to: "(ref $o)", tarr: "(ref $arr)",
  teq: "(ref eq)", tstruct: "(ref struct)", tarray: "(ref array)", tany: "(ref any)", tnone: "(ref null none)", tnn: "(ref none)", tnullany: "(ref null any)",
};
const testWat = Object.entries(testFns).map(([name, type]) => ` (func (export "${name}") (param i32) (result i32) (ref.test ${type} (call $mk (local.get 0))))`).join("\n");
const K = prefixOf(String.raw`(module
 (type $s (sub (struct (field i32))))
 (type $b (sub $s (struct (field i32) (field i32))))
 (type $o (sub (struct (field i32))))
 (type $arr (array i32))
 (func $mk (param i32) (result anyref) ${mkChain(mkCases)})
 (func $mke (param i32) (result eqref) (ref.cast (ref null eq) (call $mk (local.get 0))))
${testWat}
 (func (export "cast_s") (param i32) (result i32) (drop (ref.cast (ref $s) (call $mk (local.get 0)))) (i32.const 1))
 (func (export "cast_ns") (param i32) (result i32) (drop (ref.cast (ref null $s) (call $mk (local.get 0)))) (i32.const 1))
 (func (export "cast_b") (param i32) (result i32) (drop (ref.cast (ref $b) (call $mk (local.get 0)))) (i32.const 1))
 (func (export "cast_arr") (param i32) (result i32) (drop (ref.cast (ref $arr) (call $mk (local.get 0)))) (i32.const 1))
 (func (export "cast_struct") (param i32) (result i32) (drop (ref.cast (ref struct) (call $mk (local.get 0)))) (i32.const 1))
 (func (export "cast_eq") (param i32) (result i32) (drop (ref.cast (ref eq) (call $mk (local.get 0)))) (i32.const 1))
 (func (export "cast_i31s") (param i32) (result i32) (i31.get_s (ref.cast (ref i31) (call $mk (local.get 0)))))
 (func (export "cast_i31u") (param i32) (result i32) (i31.get_u (ref.cast (ref i31) (call $mk (local.get 0)))))
 (func (export "i31s") (param i32) (result i32) (i31.get_s (ref.i31 (local.get 0))))
 (func (export "i31u") (param i32) (result i32) (i31.get_u (ref.i31 (local.get 0))))
 (func (export "i31null") (result i32) (i31.get_s (ref.null i31)))
 (func (export "i31nullu") (result i32) (i31.get_u (ref.null i31)))
 (func (export "boc_b") (param i32) (result i32)
   (block $l (result (ref $b)) (br_on_cast $l anyref (ref $b) (call $mk (local.get 0))) drop (return (i32.const -1)))
   (struct.get $b 1))
 (func (export "boc_nb") (param i32) (result i32)
   (block $l (result (ref null $b)) (br_on_cast $l anyref (ref null $b) (call $mk (local.get 0))) drop (return (i32.const -1)))
   (ref.is_null))
 (func (export "bocf_b") (param i32) (result i32)
   (block $l (result anyref) (br_on_cast_fail $l anyref (ref $b) (call $mk (local.get 0))) (struct.get $b 1) (return))
   drop (i32.const -1))
 (func (export "bocf_i31") (param i32) (result i32)
   (block $l (result anyref) (br_on_cast_fail $l anyref (ref i31) (call $mk (local.get 0))) (i31.get_s) (return))
   drop (i32.const -1))
 (func (export "boc_i31") (param i32) (result i32)
   (block $l (result (ref i31)) (br_on_cast $l anyref (ref i31) (call $mk (local.get 0))) drop (return (i32.const -1)))
   (i31.get_u))
 (func (export "br_non_null") (param i32) (result i32)
   (block $l (result (ref any)) (br_on_non_null $l (call $mk (local.get 0))) (return (i32.const 0)))
   drop (i32.const 1))
 (func (export "eq") (param i32 i32) (result i32) (ref.eq (call $mke (local.get 0)) (call $mke (local.get 1))))
 (func (export "eq_self") (param i32) (result i32) (local $e eqref) (local.set $e (call $mke (local.get 0))) (ref.eq (local.get $e) (local.get $e)))
 (func (export "ext_roundtrip") (param externref) (result externref) (extern.convert_any (any.convert_extern (local.get 0))))
 (func (export "ext_i31") (param externref) (result i32) (ref.test (ref i31) (any.convert_extern (local.get 0))))
 (func (export "ext_null") (param externref) (result i32) (ref.is_null (any.convert_extern (local.get 0))))
 (func (export "ext_i31get") (param externref) (result i32) (i31.get_s (ref.cast (ref i31) (any.convert_extern (local.get 0)))))
 (func (export "ext_struct") (param externref) (result i32) (ref.test (ref struct) (any.convert_extern (local.get 0))))
 (func (export "any_id") (param anyref) (result anyref) (local.get 0))
 (func (export "any_isnull") (param anyref) (result i32) (ref.is_null (local.get 0)))
 (func (export "any_i31") (param anyref) (result i32) (ref.test (ref i31) (local.get 0)))
 (func (export "mkany") (param i32) (result anyref) (call $mk (local.get 0)))
 (func (export "mkext") (param i32) (result externref) (extern.convert_any (call $mk (local.get 0))))
 (func (export "eq_param") (param eqref eqref) (result i32) (ref.eq (local.get 0) (local.get 1)))
)`);
const ks = mkCases.map((_, i) => i);
for (const name of Object.keys(testFns)) calls(K, name, one(ks.concat([-1])));
calls(K, "cast_s", one(ks));
calls(K, "cast_ns", one([0, 2, 3, 4, 7]));
calls(K, "cast_b", one(ks));
calls(K, "cast_arr", one([0, 4, 2]));
calls(K, "cast_struct", one([0, 1, 2, 4]));
calls(K, "cast_eq", one([0, 1, 2, 4, 5]));
calls(K, "cast_i31s", one([0, 1, 2, 6, 8, 9]));
calls(K, "cast_i31u", one([1, 6, 8, 9]));
calls(K, "i31s", one([0, 1, -1, 1073741823, 1073741824, -1073741824, -1073741825, 2147483647, -2147483648, 4294967295, 2147483648]));
calls(K, "i31u", one([0, 1, -1, 1073741823, 1073741824, -1073741824, 2147483647, -2147483648, 4294967295]));
calls(K, "i31null", [[]]);
calls(K, "i31nullu", [[]]);
calls(K, "boc_b", one(ks));
calls(K, "boc_nb", one([0, 2, 3]));
calls(K, "bocf_b", one(ks));
calls(K, "bocf_i31", one([0, 1, 2, 6]));
calls(K, "boc_i31", one([0, 1, 6, 2]));
calls(K, "br_non_null", one([0, 1, 5]));
calls(K, "eq", [[0, 0], [1, 1], [2, 2], [2, 3], [1, 8], [1, 6], [6, 6], [0, 1], [5, 0], [4, 4]]);
calls(K, "eq_self", one([0, 1, 2, 4]));
const externs = ["undefined", "null", "1", "-1", "1.5", "2 ** 30", "2 ** 31", "-(2 ** 30)", "-(2 ** 30) - 1", "'s'", "{}", "Symbol.iterator", "1n", "true", "NaN", "-0", "x.mk ? x.mk : 0"];
for (const e of externs) {
  add(K + `L(C(() => x.ext_roundtrip(${e}) === (${e})))`);
  add(K + `L(C(() => x.ext_i31(${e})))`);
}
for (const e of ["null", "undefined", "0"]) add(K + `L(C(() => x.ext_null(${e})))`);
for (const e of ["0", "5", "-7", "1073741823", "2 ** 30", "{}"]) add(K + `L(C(() => x.ext_i31get(${e})))`);
add(K + "L(C(() => x.ext_struct(x.mkext(2)))); L(C(() => x.ext_struct(x.mkext(4)))); L(C(() => x.ext_struct(x.mkext(1))))");
add(K + "var s = x.mkany(2); L(typeof s); L(x.any_id(s) === s); L(C(() => x.any_i31(s)))");
add(K + "L(x.mkany(0)); L(x.mkany(1)); L(x.mkany(6)); L(x.mkany(8)); L(x.mkany(9)); L(typeof x.mkany(2))");
add(K + "L(x.any_id(null)); L(x.any_id(5)); L(x.any_id(-5)); L(x.any_id(1.5)); L(x.any_id('s')); L(x.any_id(undefined)); L(x.any_id(2 ** 30)); L(x.any_id(true))");
add(K + "L(C(() => x.any_i31(5))); L(C(() => x.any_i31(2 ** 30))); L(C(() => x.any_i31('s'))); L(C(() => x.any_i31(null))); L(C(() => x.any_isnull(null))); L(C(() => x.any_isnull(0)))");
add(K + "var a = x.mkany(2); L(C(() => x.eq_param(a, a))); L(C(() => x.eq_param(a, x.mkany(2)))); L(C(() => x.eq_param(1, 1))); L(C(() => x.eq_param(null, null))); L(C(() => x.eq_param('s', 's')))");
add(K + "L(C(() => x.eq_param(1.5, 1.5))); L(C(() => x.eq_param(2 ** 30, 2 ** 30))); L(C(() => x.eq_param({}, {})))");
add(K + "var a = x.mkext(2); L(typeof a); L(x.ext_roundtrip(a) === a); L(typeof x.mkext(1)); L(x.mkext(0))");

// ---------- subtipagem e rec groups ----------
const R = prefixOf(String.raw`(module
 (rec (type $a1 (struct (field (mut (ref null $b1))))) (type $b1 (struct (field (mut (ref null $a1))))))
 (rec (type $a2 (struct (field (mut (ref null $b2))))) (type $b2 (struct (field (mut (ref null $a2))))))
 (rec (type $a3 (struct (field (mut (ref null $b3))))) (type $b3 (struct (field (mut (ref null $b3))))))
 (type $fin (struct (field i32)))
 (type $nfin (sub (struct (field i32))))
 (type $fin2 (struct (field i32)))
 (type $d0 (sub (struct)))
 (type $d1 (sub $d0 (struct (field i32))))
 (type $d2 (sub $d1 (struct (field i32) (field i32))))
 (type $d3 (sub $d2 (struct (field i32) (field i32) (field i32))))
 (type $fx (sub final $d0 (struct (field i64))))
 (type $arrm (array (mut i32)))
 (type $arri (array i32))
 (type $arrm2 (array (mut i32)))
 (type $arrp (array (mut i8)))
 (type $arrq (array (mut i16)))
 (func $mkd (param i32) (result anyref)
   (if (result anyref) (i32.eq (local.get 0) (i32.const 0)) (then (struct.new_default $d0))
   (else (if (result anyref) (i32.eq (local.get 0) (i32.const 1)) (then (struct.new_default $d1))
   (else (if (result anyref) (i32.eq (local.get 0) (i32.const 2)) (then (struct.new_default $d2))
   (else (if (result anyref) (i32.eq (local.get 0) (i32.const 3)) (then (struct.new_default $d3)) (else (struct.new_default $fx))))))))))
 (func (export "t_d0") (param i32) (result i32) (ref.test (ref $d0) (call $mkd (local.get 0))))
 (func (export "t_d1") (param i32) (result i32) (ref.test (ref $d1) (call $mkd (local.get 0))))
 (func (export "t_d2") (param i32) (result i32) (ref.test (ref $d2) (call $mkd (local.get 0))))
 (func (export "t_d3") (param i32) (result i32) (ref.test (ref $d3) (call $mkd (local.get 0))))
 (func (export "t_fx") (param i32) (result i32) (ref.test (ref $fx) (call $mkd (local.get 0))))
 (func (export "c_d2") (param i32) (result i32) (drop (ref.cast (ref $d2) (call $mkd (local.get 0)))) (i32.const 1))
 (func (export "same_rec") (result i32) (ref.test (ref $a2) (struct.new_default $a1)))
 (func (export "same_rec_b") (result i32) (ref.test (ref $b2) (struct.new_default $b1)))
 (func (export "cross_rec") (result i32) (ref.test (ref $b2) (struct.new_default $a1)))
 (func (export "diff_rec") (result i32) (ref.test (ref $a3) (struct.new_default $a1)))
 (func (export "diff_rec_b") (result i32) (ref.test (ref $b3) (struct.new_default $b1)))
 (func (export "fin_vs_nfin") (result i32) (ref.test (ref $nfin) (struct.new $fin (i32.const 1))))
 (func (export "nfin_vs_fin") (result i32) (ref.test (ref $fin) (struct.new $nfin (i32.const 1))))
 (func (export "fin_vs_fin2") (result i32) (ref.test (ref $fin2) (struct.new $fin (i32.const 1))))
 (func (export "arr_same") (result i32) (ref.test (ref $arrm2) (array.new_default $arrm (i32.const 1))))
 (func (export "arr_imm") (result i32) (ref.test (ref $arri) (array.new_default $arrm (i32.const 1))))
 (func (export "arr_pack") (result i32) (ref.test (ref $arrq) (array.new_default $arrp (i32.const 1))))
 (func (export "cyc") (result i32) (local $a (ref null $a1)) (local $b (ref null $b1))
   (local.set $a (struct.new_default $a1)) (local.set $b (struct.new_default $b1))
   (struct.set $a1 0 (local.get $a) (local.get $b)) (struct.set $b1 0 (local.get $b) (local.get $a))
   (ref.eq (struct.get $b1 0 (struct.get $a1 0 (local.get $a))) (local.get $b)))
 (func (export "cyc_null") (result i32) (ref.is_null (struct.get $b1 0 (struct.get $a1 0 (struct.new_default $a1)))))
 (func (export "mka") (result (ref $a1)) (struct.new_default $a1))
 (func (export "mka2") (result (ref $a2)) (struct.new_default $a2))
 (func (export "usea2") (param (ref $a2)) (result i32) (i32.const 1))
 (func (export "usea1") (param (ref $a1)) (result i32) (i32.const 1))
 (func (export "used0") (param (ref $d0)) (result i32) (i32.const 1))
 (func (export "mkd") (param i32) (result (ref $d0)) (ref.cast (ref $d0) (call $mkd (local.get 0))))
)`);
for (const f of ["t_d0", "t_d1", "t_d2", "t_d3", "t_fx"]) calls(R, f, one([0, 1, 2, 3, 4]));
calls(R, "c_d2", one([0, 1, 2, 3, 4]));
for (const f of ["same_rec", "same_rec_b", "cross_rec", "diff_rec", "diff_rec_b", "fin_vs_nfin", "nfin_vs_fin", "fin_vs_fin2", "arr_same", "arr_imm", "arr_pack", "cyc", "cyc_null"]) calls(R, f, [[]]);
add(R + "L(C(() => x.usea2(x.mka()))); L(C(() => x.usea1(x.mka2()))); L(C(() => x.usea1(x.mka())))");
add(R + "L(C(() => x.used0(x.mkd(0)))); L(C(() => x.used0(x.mkd(2)))); L(C(() => x.used0(x.mkd(4)))); L(C(() => x.used0(x.mka())))");
// Dois módulos com o mesmo rec group: o JS passa a instância de um para a função do outro.
const R2 = hexOf(String.raw`(module
 (rec (type $a (struct (field (mut (ref null $b))))) (type $b (struct (field (mut (ref null $a))))))
 (func (export "mk") (result (ref $a)) (struct.new_default $a))
 (func (export "use") (param (ref $a)) (result i32) (i32.const 1)))`);
const R3 = hexOf(String.raw`(module
 (rec (type $a (struct (field (mut (ref null $b))))) (type $b (struct (field (mut (ref null $b))))))
 (func (export "use") (param (ref $a)) (result i32) (i32.const 1)))`);
add(`var p = RUN(HX("${R2}")), q = RUN(HX("${R2}")); L(C(() => q.use(p.mk()))); L(C(() => p.use(q.mk())))`);
add(`var p = RUN(HX("${R2}")), q = RUN(HX("${R3}")); L(C(() => q.use(p.mk()))); L(C(() => p.use(p.mk())))`);

// ---------- tabelas, funcref e externref, return_call ----------
const T = prefixOf(String.raw`(module
 (type $ft (func (result i32)))
 (type $ft2 (func (param i32) (result i32)))
 (table $t 4 10 funcref)
 (table $e 2 6 externref)
 (elem (table $t) (i32.const 0) func $f0 $f1 $f2)
 (elem $pe func $f1 $f2)
 (elem declare func $f0 $f1 $f2)
 (func $f0 (type $ft) i32.const 10)
 (func $f1 (type $ft) i32.const 11)
 (func $f2 (type $ft) i32.const 12)
 (func $cnt (param i32) (result i32) (if (i32.eqz (local.get 0)) (then (return (i32.const 7)))) (return_call $cnt (i32.sub (local.get 0) (i32.const 1))))
 (func $cnti (param i32) (result i32) (if (i32.eqz (local.get 0)) (then (return (i32.const 8))))
   (return_call_indirect $t (type $ft2) (i32.sub (local.get 0) (i32.const 1)) (i32.const 3)))
 (func $rec (param i32) (result i32) (i32.add (call $rec (local.get 0)) (i32.const 1)))
 (func $sum (param i32 i64) (result i64) (if (result i64) (i32.eqz (local.get 0)) (then (local.get 1)) (else (return_call $sum (i32.sub (local.get 0) (i32.const 1)) (i64.add (local.get 1) (i64.extend_i32_u (local.get 0)))))))
 (elem (table $t) (i32.const 3) func $cnti)
 (func (export "size_t") (result i32) (table.size $t))
 (func (export "size_e") (result i32) (table.size $e))
 (func (export "grow_t") (param i32) (result i32) (table.grow $t (ref.null func) (local.get 0)))
 (func (export "grow_ti") (param i32) (result i32) (table.grow $t (ref.func $f1) (local.get 0)))
 (func (export "call") (param i32) (result i32) (call_indirect $t (type $ft) (local.get 0)))
 (func (export "calli") (param i32) (result i32) (call_indirect $t (type $ft2) (i32.const 5) (local.get 0)))
 (func (export "fill_t") (param i32 i32) (result i32) (table.fill $t (local.get 0) (ref.func $f2) (local.get 1)) (call_indirect $t (type $ft) (local.get 0)))
 (func (export "copy_t") (param i32 i32 i32) (result i32) (table.copy $t $t (local.get 0) (local.get 1) (local.get 2)) (call_indirect $t (type $ft) (local.get 0)))
 (func (export "init_t") (param i32 i32 i32) (result i32) (table.init $t $pe (local.get 0) (local.get 1) (local.get 2)) (call_indirect $t (type $ft) (local.get 0)))
 (func (export "get_t") (param i32) (result funcref) (table.get $t (local.get 0)))
 (func (export "set_t") (param i32) (table.set $t (local.get 0) (ref.func $f0)))
 (func (export "set_tn") (param i32) (table.set $t (local.get 0) (ref.null func)))
 (func (export "grow_e") (param i32 externref) (result i32) (table.grow $e (local.get 1) (local.get 0)))
 (func (export "get_e") (param i32) (result externref) (table.get $e (local.get 0)))
 (func (export "set_e") (param i32 externref) (table.set $e (local.get 0) (local.get 1)))
 (func (export "fill_e") (param i32 externref i32) (table.fill $e (local.get 0) (local.get 1) (local.get 2)))
 (func (export "copy_e") (param i32 i32 i32) (table.copy $e $e (local.get 0) (local.get 1) (local.get 2)))
 (func (export "cnt") (param i32) (result i32) (call $cnt (local.get 0)))
 (func (export "cnti") (param i32) (result i32) (call $cnti (local.get 0)))
 (func (export "rec") (param i32) (result i32) (call $rec (local.get 0)))
 (func (export "sum") (param i32) (result i64) (call $sum (local.get 0) (i64.const 0)))
 (func (export "isnull_t") (param i32) (result i32) (ref.is_null (table.get $t (local.get 0))))
 (func (export "is_f1") (param i32) (result i32) (ref.eq (ref.cast (ref null eq) (ref.null none)) (ref.null none)))
 (export "tab" (table $t))
 (export "tabe" (table $e))
 (func (export "reffunc") (result funcref) (ref.func $f2))
 (func (export "typed") (param (ref null $ft)) (result i32) (call_ref $ft (local.get 0)))
 (func (export "mkfn") (result (ref $ft)) (ref.func $f1))
)`);
calls(T, "size_t", [[]]);
calls(T, "size_e", [[]]);
calls(T, "grow_t", one([0, 1, 7, 8, 11, -1, 4294967295]));
calls(T, "grow_ti", one([0, 1, 7, 8]));
calls(T, "call", one([0, 1, 2, 3, 4, -1, "'1'"]));
calls(T, "calli", one([0]));
calls(T, "fill_t", [[0, 3], [1, 2], [2, 1], [2, 2], [3, 0], [3, 1], [0, 0], [4, 0], [0, 4]]);
calls(T, "copy_t", [[0, 1, 2], [1, 0, 2], [0, 0, 3], [0, 2, 1], [2, 0, 2], [3, 0, 0], [4, 0, 0], [0, 4, 0], [0, 3, 2]]);
calls(T, "init_t", [[0, 0, 2], [0, 1, 1], [1, 0, 2], [0, 2, 0], [0, 3, 0], [2, 1, 2], [3, 0, 1], [0, 0, 3], [4, 0, 0]]);
calls(T, "get_t", one([0, 1, 3, -1]), (c) => `var f = ${c}; L(C(() => typeof f)); L(C(() => f === null ? 'null' : f()))`);
calls(T, "get_t", one([0, 2]), (c) => `L(C(() => (${c}).name)); L(C(() => (${c}).length))`);
calls(T, "set_t", one([0, 2, 3, -1]));
calls(T, "set_tn", one([0, 2, 3]));
add(T + "x.set_tn(1); L(C(() => x.call(1))); L(x.isnull_t(1)); L(x.isnull_t(0))");
calls(T, "grow_e", [[0, "{}"], [1, "{}"], [4, "'a'"], [5, "1"], [-1, "null"], [3, "undefined"]]);
add(T + "L(C(() => x.get_e(0))); L(C(() => x.get_e(1))); L(C(() => x.get_e(2))); L(C(() => x.get_e(-1)))");
add(T + "var o = {}; x.set_e(0, o); L(x.get_e(0) === o); x.set_e(1, 5); L(x.get_e(1)); L(C(() => x.set_e(2, o))); L(C(() => x.set_e(-1, o)))");
add(T + "var o = {}; x.grow_e(2, o); L(x.get_e(2) === o); L(x.get_e(3) === o); L(x.get_e(1)); L(x.size_e())");
add(T + "var o = {}; L(C(() => x.fill_e(0, o, 2))); L(x.get_e(1) === o); L(C(() => x.fill_e(1, o, 2))); L(C(() => x.fill_e(2, o, 0))); L(C(() => x.fill_e(3, o, 0)))");
add(T + "var o = {}; x.set_e(0, o); L(C(() => x.copy_e(1, 0, 1))); L(x.get_e(1) === o); L(C(() => x.copy_e(0, 1, 2))); L(C(() => x.copy_e(2, 0, 0))); L(C(() => x.copy_e(3, 0, 0)))");
add(T + "L(x.tab.length); L(x.tab.get(0)()); L(x.tab.get(2)()); L(C(() => x.tab.get(5))); L(x.tab.grow(1)); L(x.tab.length); L(x.tab.get(3)); L(C(() => x.size_t()))");
add(T + "L(C(() => x.tab.grow(100))); L(x.tab.grow(7)); L(C(() => x.tab.grow(1))); L(x.size_t())");
add(T + "x.tab.set(0, x.reffunc()); L(x.call(0)); L(C(() => x.tab.set(0, () => 1))); L(C(() => x.tab.set(0, null))); L(C(() => x.call(0)))");
add(T + "L(x.tabe.length); L(x.tabe.get(0)); var o = {}; x.tabe.set(1, o); L(x.get_e(1) === o); L(x.tabe.grow(2, 'z')); L(x.tabe.get(3)); L(C(() => x.tabe.grow(9)))");
add(T + "L(x.typed(x.mkfn())); L(C(() => x.typed(null))); L(C(() => x.typed(x.reffunc()))); L(C(() => x.typed(() => 1))); L(C(() => x.typed(x.cnt)))");
add(T + "L(x.mkfn() === x.mkfn()); L(typeof x.mkfn()); L(x.reffunc() === x.tab.get(2)); L(x.reffunc()())");
for (const n of [0, 1, 10, 100000, 1000000, 10000000]) add(T + `L(C(() => x.cnt(${n})))`);
for (const n of [0, 1, 2, 3, 4, 5, 6]) add(T + `L(C(() => x.cnti(${n})))`);
add(T + "L(C(() => x.rec(0)))");
add(T + "L(C(() => String(x.sum(10)))); L(C(() => String(x.sum(100000)))); L(C(() => String(x.sum(0))))");

// ---------- exceções com try_table ----------
const E = prefixOf(String.raw`(module
 (type $fv (func (param i32)))
 (import "m" "f" (func $imp (param i32)))
 (tag $t (param i32))
 (tag $m (param i32 i64 f64))
 (tag $n)
 (func $throw1 (param i32) (throw $t (local.get 0)))
 (func (export "throw1") (param i32) (throw $t (local.get 0)))
 (func (export "throwm") (param i32 i64 f64) (throw $m (local.get 0) (local.get 1) (local.get 2)))
 (func (export "thrown") (throw $n))
 (func (export "catch1") (param i32) (result i32)
   (block $h (result i32) (try_table (result i32) (catch $t $h) (throw $t (local.get 0)))))
 (func (export "catch1ok") (param i32) (result i32)
   (block $h (result i32) (try_table (result i32) (catch $t $h) (local.get 0))))
 (func (export "catchn") (result i32)
   (block $h (try_table (catch $n $h) (throw $n)) (return (i32.const 0))) (i32.const 1))
 (func (export "catchm") (param i32 i64 f64) (result i32 i64 f64)
   (block $h (result i32 i64 f64) (try_table (catch $m $h) (throw $m (local.get 0) (local.get 1) (local.get 2))) (unreachable)))
 (func (export "catchall") (param i32) (result i32)
   (block $h (try_table (catch_all $h) (call $throw1 (local.get 0))) (return (i32.const 0))) (i32.const 1))
 (func (export "catchall_trap") (result i32)
   (block $h (try_table (catch_all $h) (unreachable)) (return (i32.const 0))) (i32.const 1))
 (func (export "catchall_div") (param i32) (result i32)
   (block $h (try_table (catch_all $h) (drop (i32.div_s (i32.const 1) (local.get 0)))) (return (i32.const 0))) (i32.const 1))
 (func (export "wrongtag") (param i32) (result i32)
   (block $h (try_table (catch $n $h) (throw $t (local.get 0))) (return (i32.const 0))) (i32.const 1))
 (func (export "catchref_rethrow") (param i32) (result i32)
   (block $h (result i32 exnref) (try_table (catch_ref $t $h) (throw $t (local.get 0))) (return (i32.const -1)))
   (throw_ref))
 (func (export "catchallref_rethrow") (param i32) (result i32)
   (block $h (result exnref) (try_table (catch_all_ref $h) (call $throw1 (local.get 0))) (return (i32.const -1)))
   (throw_ref))
 (func (export "throwref_null") (throw_ref (ref.null exn)))
 (func (export "nested") (param i32) (result i32)
   (block $outer (result i32)
     (try_table (catch $t $outer)
       (block $inner (try_table (catch $n $inner) (throw $t (i32.add (local.get 0) (i32.const 100))))))
     (i32.const -3)))
 (func (export "two") (param i32) (result i32)
   (block $c
     (block $b (result i32)
       (try_table (catch $t $b) (catch_all $c) (if (local.get 0) (then (throw $t (i32.const 5))) (else (throw $n))))
       (unreachable))
     (i32.const 1000) (i32.add) (return))
   (i32.const 2000))
 (func (export "calljs") (param i32) (result i32)
   (block $h (try_table (catch_all $h) (call $imp (local.get 0))) (return (i32.const 0))) (i32.const 1))
 (func (export "calljs_tag") (param i32) (result i32)
   (block $h (result i32) (try_table (result i32) (catch $t $h) (call $imp (local.get 0)) (i32.const 0))))
 (func (export "calljs_ref") (param i32) (result i32)
   (block $h (result exnref) (try_table (catch_all_ref $h) (call $imp (local.get 0))) (return (i32.const 0)))
   (throw_ref))
 (func (export "calljs_through") (param i32) (call $imp (local.get 0)))
 (export "t" (tag $t))
 (export "m" (tag $m))
 (export "n" (tag $n))
)`, "{ m: { f() {} } }");
calls(E, "throw1", one([0, 5, -1, 2147483647, 4294967295, 1.5, "'3'"]), (c) => `L(C(() => ${c}))`);
add(E + "try { x.throw1(5) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(x.t)); L(e.is(x.n)); L(e.getArg(x.t, 0)) }");
add(E + "try { x.throw1(-9) } catch (e) { L(e.getArg(x.t, 0)); L(T(() => e.getArg(x.n, 0))); L(T(() => e.getArg(x.t, 1))) }");
add(E + "try { x.throwm(1, 2n, 3.5) } catch (e) { L(e.is(x.m)); L(e.getArg(x.m, 0)); L(String(e.getArg(x.m, 1))); L(e.getArg(x.m, 2)); L(T(() => e.getArg(x.m, 3))) }");
add(E + "try { x.throwm(1, 2, 3.5) } catch (e) { L(X(e)) }");
add(E + "try { x.throwm(1, 2n) } catch (e) { L(X(e)) }");
add(E + "try { x.throwm(1.9, -(2n ** 63n), NaN) } catch (e) { L(e.getArg(x.m, 0)); L(String(e.getArg(x.m, 1))); L(e.getArg(x.m, 2)) }");
add(E + "try { x.thrown() } catch (e) { L(e.is(x.n)); L(e.is(x.t)); L(T(() => e.getArg(x.n, 0))) }");
calls(E, "catch1", one([0, 5, -1, 2147483647]));
calls(E, "catch1ok", one([0, 5]));
calls(E, "catchn", [[]]);
calls(E, "catchm", [[1, "2n", 3.5], [-1, "-1n", "-0"], [0, "2n ** 63n", "NaN"]]);
calls(E, "catchall", one([0, 5, -1]));
calls(E, "catchall_trap", [[]]);
calls(E, "catchall_div", one([0, 1, -1]));
calls(E, "wrongtag", one([0, 5]));
calls(E, "catchref_rethrow", one([0, 5, -1]));
calls(E, "catchallref_rethrow", one([0, 5]));
calls(E, "throwref_null", [[]]);
calls(E, "nested", one([0, 5, -1]));
calls(E, "two", one([0, 1]));
add(E + "try { x.catchref_rethrow(8) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(x.t)); L(e.getArg(x.t, 0)) }");
add(E + "try { x.catchallref_rethrow(8) } catch (e) { L(e instanceof WebAssembly.Exception); L(e.is(x.t)); L(e.getArg(x.t, 0)) }");
add(E + "try { x.nested(1) } catch (e) { L(e.is(x.t)); L(e.getArg(x.t, 0)) }");
add(E + "try { x.wrongtag(3) } catch (e) { L(e.is(x.t)); L(e.getArg(x.t, 0)) }");
add(E + "L(x.t instanceof WebAssembly.Tag); L(x.t === x.t); L(x.t === x.n); L(Object.prototype.toString.call(x.m))");
add(E + "var e = new WebAssembly.Exception(x.t, [42]); L(e.is(x.t)); L(e.getArg(x.t, 0)); L(e instanceof Error)");
add(E + "var e = new WebAssembly.Exception(x.m, [1, 2n, 3.5]); L(e.getArg(x.m, 0)); L(String(e.getArg(x.m, 1))); L(e.getArg(x.m, 2)); L(T(() => new WebAssembly.Exception(x.m, [1])))");
// Imports de JS lançando coisas para o try_table.
const impHex = E.match(/HX\("([0-9a-f]+)"\)/)[1];
for (const body of ["{}", "{ throw new Error('boom') }", "{ throw 42 }", "{ throw new WebAssembly.Exception(x.t, [7]) }", "{ throw undefined }", "{ throw null }", "{ throw { a: 1 } }", "{ throw new WebAssembly.Exception(x.n, []) }"]) {
  const p = `var x, f = v => ${body}; x = RUN(HX("${impHex}"), { m: { f: v => f(v) } }); `;
  for (const fn of ["calljs", "calljs_tag", "calljs_ref", "calljs_through"]) add(p + `L(C(() => x.${fn}(5)))`);
}
add(E + "L(C(() => x.calljs(1)))");
add(`var x = RUN(HX("${impHex}"), { m: { f: v => { throw new WebAssembly.Exception(x.t, [v * 2]) } } }); L(C(() => x.calljs_tag(4))); L(C(() => x.calljs(4)))`);
add(`var x = RUN(HX("${impHex}"), { m: { f: v => { throw new WebAssembly.Exception(x.t, [v * 2]) } } }); try { x.calljs_through(4) } catch (e) { L(e.is(x.t)); L(e.getArg(x.t, 0)) }`);
add(`var x = RUN(HX("${impHex}"), { m: { f: v => { throw new RangeError('r') } } }); try { x.calljs_ref(4) } catch (e) { L(e instanceof RangeError); L(E(e)) }`);

// ---------- multi-valor ----------
const V = prefixOf(String.raw`(module
 (type $three (func (param i32) (result i32 i64 f64)))
 (type $pair (func (param i32 i32) (result i32 i32)))
 (import "m" "f" (func $imp (param i32) (result i32 i32)))
 (table 2 funcref)
 (elem (i32.const 0) $swap $three_f)
 (func $swap (type $pair) (local.get 1) (local.get 0))
 (func $three_f (type $three) (local.get 0) (i64.extend_i32_s (local.get 0)) (f64.convert_i32_s (local.get 0)))
 (func (export "swap") (type $pair) (local.get 1) (local.get 0))
 (func (export "three") (type $three) (local.get 0) (i64.extend_i32_s (local.get 0)) (f64.convert_i32_s (local.get 0)))
 (func (export "none"))
 (func (export "one") (result i32) (i32.const 1))
 (func (export "refs") (param externref) (result externref externref i32) (local.get 0) (ref.null extern) (i32.const 3))
 (func (export "mixed") (param i32) (result anyref funcref i32) (ref.i31 (local.get 0)) (ref.func $swap) (local.get 0))
 (func (export "block") (param i32 i32) (result i32) (local.get 0) (local.get 1) (block (param i32 i32) (result i32) (i32.add)))
 (func (export "block2") (param i32 i32) (result i32 i32) (local.get 0) (local.get 1) (block (param i32 i32) (result i32 i32) (i32.add) (i32.const 100)))
 (func (export "loop") (param i32) (result i32 i32) (local i32) (local.get 0) (i32.const 0)
   (loop $l (param i32 i32) (result i32 i32)
     (local.set 0) (local.set 1) (local.get 1) (i32.const 1) (i32.add) (local.get 0) (i32.const 2) (i32.add)
     (local.get 1) (i32.const 3) (i32.lt_s) (br_if $l)))
 (func (export "br_multi") (param i32) (result i32 i32)
   (block $b (result i32 i32) (i32.const 1) (i32.const 2) (br_if $b (local.get 0)) (drop) (drop) (i32.const 8) (i32.const 9)))
 (func (export "callind") (param i32) (result i32 i32) (local.get 0) (i32.const 6) (i32.const 0) (call_indirect (type $pair)))
 (func (export "callind_bad") (param i32) (result i32 i32) (local.get 0) (i32.const 6) (i32.const 1) (call_indirect (type $pair)))
 (func (export "retcall") (param i32) (result i32 i64 f64) (return_call $three_f (local.get 0)))
 (func (export "retcall_ind") (param i32 i32) (result i32 i32) (local.get 0) (local.get 1) (i32.const 0) (return_call_indirect (type $pair)))
 (func (export "viaimp") (param i32) (result i32 i32) (call $imp (local.get 0)))
 (func (export "imp_swap") (param i32) (result i32) (call $imp (local.get 0)) (i32.sub))
)`, "{ m: { f: v => [v, v + 1] } }");
const vHex = V.match(/HX\("([0-9a-f]+)"\)/)[1];
const withImp = (jsf) => `var x = RUN(HX("${vHex}"), { m: { f: ${jsf} } }); `;
calls(V, "swap", [[1, 2], [0, -1], [2147483647, 4294967295], [1.5, "'3'"], [1], []]);
calls(V, "three", one([0, 5, -5, 2147483647, 4294967295, 1.5]), (c) => `var r = ${c}; L(Array.isArray(r)); L(r.length); L(String(r[0])); L(String(r[1])); L(String(r[2]))`);
calls(V, "three", [[3]], (c) => `var r = ${c}; L(typeof r[1]); L(Object.getPrototypeOf(r) === Array.prototype); L(Object.isFrozen(r)); L(r.constructor === Array)`);
add(V + "L(x.none()); L(x.one()); L(Array.isArray(x.none())); L(Array.isArray(x.one()))");
add(V + "var o = {}; var r = x.refs(o); L(r[0] === o); L(r[1]); L(r[2]); var r2 = x.refs(5); L(r2[0]); L(x.refs(null)[0])");
add(V + "var r = x.mixed(5); L(r.length); L(r[0]); L(typeof r[1]); L(r[2]); L(x.mixed(2 ** 30)[0]); L(x.mixed(-1)[0])");
calls(V, "block", [[1, 2], [-1, 1], [2147483647, 1]]);
calls(V, "block2", [[1, 2], [5, 5]], (c) => `L(C(() => ${c}))`);
calls(V, "loop", one([0, 1, 2, 3, 5]), (c) => `L(C(() => ${c}))`);
calls(V, "br_multi", one([0, 1, 2]), (c) => `L(C(() => ${c}))`);
calls(V, "callind", one([1, 2]), (c) => `L(C(() => ${c}))`);
calls(V, "callind_bad", one([1]), (c) => `L(C(() => ${c}))`);
calls(V, "retcall", one([4, -4]), (c) => `L(C(() => ${c}))`);
calls(V, "retcall_ind", [[1, 2], [7, 8]], (c) => `L(C(() => ${c}))`);
const impCases = {
  "[v, v + 1]": 5, "[1, 2, 3]": 5, "[1]": 5, "[]": 5, "new Set([3, 4])": 5, "'ab'": 5, "(function* () { yield 1; yield 2 })()": 5,
  "({ length: 2, 0: 1, 1: 2 })": 5, "undefined": 5, "5": 5, "null": 5, "['1', '2']": 5, "[1.5, -1.5]": 5, "[2 ** 32, 2 ** 31]": 5, "[1n, 2n]": 5,
};
for (const jsf of Object.keys(impCases)) {
  add(withImp(`v => ${jsf}`) + "L(C(() => x.viaimp(5)))");
  add(withImp(`v => ${jsf}`) + "L(C(() => x.imp_swap(5)))");
}
add(withImp("v => { throw new Error('m') }") + "L(C(() => x.viaimp(1)))");
add(withImp("v => [v, v]") + "L(C(() => x.viaimp(-3))); L(C(() => x.viaimp(2147483648)))");

if (programs.length < 250) throw new Error("só " + programs.length + " programas");

const lines = [];
programs.forEach((source, index) => {
  const file = path.join(tmp, `p${index}.js`);
  const script =
    harness +
    `\nprocess.on("unhandledRejection", () => {});\n__run(${JSON.stringify(source)});\nsetTimeout(() => { const out = __final(); process.stdout.write(out); }, 50);\n`;
  fs.writeFileSync(file, script);
  const run = spawnSync(process.execPath, [file], { timeout: 20000, encoding: "utf8", cwd: tmp });
  let result = run.stdout;
  if (run.error || run.status !== 0 || result === "") result = `error\tHarness\t${JSON.stringify("sem resultado do bun")}`;
  lines.push(`${source}\t${result.replace(/[\t\n\r]+$/, "")}`);
});
fs.rmSync(tmp, { recursive: true, force: true });
process.stdout.write(require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
process.stderr.write(`${programs.length} programas\n`);
