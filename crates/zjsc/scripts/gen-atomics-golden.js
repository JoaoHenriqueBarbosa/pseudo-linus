// Gera tests/golden/atomics_bun.tsv: Atomics (todas as operações, em todos os tipos inteiros, em SharedArrayBuffer e
// ArrayBuffer comum, índices e valores que pedem coerção, wait/waitAsync/notify sem bloquear) e SharedArrayBuffer,
// medidos no bun 1.4.2. Também cobre DataView (get/set de todos os tipos, inclusive Float16, as duas endianness,
// limites e mensagens de erro, sobre buffers fixos, redimensionáveis e compartilhados) e ArrayBuffer redimensionável
// (resize, transfer, transferToFixedLength, detached, typed arrays length-tracking). Complementa buffer_bun.tsv, que só tem o Atomics básico.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), como em gen-buffer-golden.js.
// Cada programa é uma expressão que o gerador embrulha em `R = F(function () { return <expr> })`, onde F serializa
// o valor ou o `Nome: mensagem` da exceção.
// Cada programa roda em um bun filho novo (no máximo 6 ao mesmo tempo, timeout de 8 s), e programas iguais aos de
// outros goldens (knownPrograms) são descartados.
// Uso: bun scripts/gen-atomics-golden.js > tests/golden/atomics_bun.tsv
const { spawn } = require("child_process");
if (process.argv[2] === "--child") {
  (0, eval)(require("fs").readFileSync(0, "utf8"));
  process.stdout.write(globalThis.R === undefined ? "<undefined>" : String(globalThis.R));
  process.exit(0);
}
const PRE =
  "function F(f){try{var v=f();return typeof v==='bigint'?v+'n':JSON.stringify(v,function(k,x){return typeof x==='bigint'?x+'n':Object.is(x,-0)?'-0':x===undefined?'undef':typeof x==='number'&&!isFinite(x)?String(x):x})}catch(e){return 'throw '+e.name+': '+e.message}}\n";

const exprs = [];
const seen = new Set();
const add = (...bodies) => {
  for (const body of bodies) {
    if (!seen.has(body)) {
      seen.add(body);
      exprs.push(body);
    }
  }
};

const intTypes = ["Int8Array", "Uint8Array", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array"];
const bigTypes = ["BigInt64Array", "BigUint64Array"];
const allTypes = [...intTypes, ...bigTypes];
const badTypes = ["Uint8ClampedArray", "Float16Array", "Float32Array", "Float64Array"];
const backings = { shared: (n, bytes) => `new SharedArrayBuffer(${bytes})`, plain: (n, bytes) => `new ArrayBuffer(${bytes})` };
const sizeOf = { Int8Array: 1, Uint8Array: 1, Int16Array: 2, Uint16Array: 2, Int32Array: 4, Uint32Array: 4, BigInt64Array: 8, BigUint64Array: 8, Uint8ClampedArray: 1, Float16Array: 2, Float32Array: 4, Float64Array: 8 };
const mk = (C, kind, n = 4) => `new ${C}(${backings[kind](n, n * sizeOf[C])})`;
const lit = (C, v) => (C.startsWith("Big") ? `${v}n` : `${v}`);
const ops2 = ["add", "and", "exchange", "or", "sub", "xor"];

// ---- Atomics: forma do objeto
add(`Object.prototype.toString.call(Atomics)`);
add(`Atomics[Symbol.toStringTag]`);
add(`Object.getOwnPropertyDescriptor(Atomics, Symbol.toStringTag)`);
add(`Object.getOwnPropertyNames(Atomics).sort()`);
add(`Object.getOwnPropertySymbols(Atomics).length`);
add(`Object.keys(Atomics)`);
add(`Object.getPrototypeOf(Atomics) === Object.prototype`);
add(`typeof Atomics`);
add(`(function(){try{return Atomics()}catch(e){return e.name+': '+e.message}})()`);
add(`(function(){try{return new Atomics()}catch(e){return e.name+': '+e.message}})()`);
add(`Object.getOwnPropertyDescriptor(globalThis, "Atomics").enumerable`);
add(`Object.getOwnPropertyDescriptor(globalThis, "Atomics").writable`);
add(`Object.getOwnPropertyDescriptor(globalThis, "Atomics").configurable`);
for (const n of [...ops2, "compareExchange", "isLockFree", "load", "notify", "store", "wait", "waitAsync", "pause"]) {
  add(`[typeof Atomics.${n}, Atomics.${n}.length, Atomics.${n}.name]`);
  add(`JSON.stringify(Object.getOwnPropertyDescriptor(Atomics, "${n}"), function(k,v){return typeof v==='function'?'fn':v})`);
  add(`Atomics.${n}.hasOwnProperty("prototype")`);
  add(`(function(){try{return new Atomics.${n}()}catch(e){return e.name+': '+e.message}})()`);
}

// ---- operações de leitura-modificação-escrita: todos os tipos, os dois buffers
for (const kind of ["shared", "plain"]) {
  for (const C of allTypes) {
    const one = lit(C, 5), three = lit(C, 3);
    for (const op of ops2) {
      add(`(function(){var a=${mk(C, kind)};a[0]=${one};a[1]=${lit(C, 12)};return [Atomics.${op}(a,0,${three}),a[0],Atomics.${op}(a,1,${lit(C, 0)}),a[1],a[2]]})()`);
    }
    add(`(function(){var a=${mk(C, kind)};a[0]=${one};return [Atomics.compareExchange(a,0,${one},${three}),a[0],Atomics.compareExchange(a,0,${one},${lit(C, 9)}),a[0]]})()`);
    add(`(function(){var a=${mk(C, kind)};a[0]=${one};return [Atomics.load(a,0),Atomics.load(a,1),Atomics.load(a,3)]})()`);
    add(`(function(){var a=${mk(C, kind)};return [Atomics.store(a,0,${three}),a[0],Atomics.store(a,3,${one}),a[3]]})()`);
  }
}

// ---- wrap-around e extremos
const ranges = {
  Int8Array: [127, -128, 255, 256, 257, -129],
  Uint8Array: [255, 0, 256, 257, -1, 511],
  Int16Array: [32767, -32768, 65535, 65536, 65537, -32769],
  Uint16Array: [65535, 0, 65536, 65537, -1, 131071],
  Int32Array: [2147483647, -2147483648, 4294967295, 4294967296, 4294967297, -2147483649],
  Uint32Array: [4294967295, 0, 4294967296, 4294967297, -1, 8589934591],
};
for (const C of intTypes) {
  for (const v of ranges[C]) {
    for (const op of ops2) {
      add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));a[0]=${v};return [Atomics.${op}(a,0,1),a[0]]})()`);
      add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));a[0]=3;return [Atomics.${op}(a,0,${v}),a[0]]})()`);
    }
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));return [Atomics.store(a,0,${v}),a[0]]})()`);
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));a[0]=${v};return [Atomics.compareExchange(a,0,${v},7),a[0]]})()`);
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));a[0]=${v};return [Atomics.compareExchange(a,0,${v}+${sizeOf[C] === 1 ? 256 : sizeOf[C] === 2 ? 65536 : 4294967296},7),a[0]]})()`);
  }
}
const bigExtremes = ["0n", "1n", "-1n", "2n**63n", "2n**63n-1n", "-(2n**63n)", "2n**64n", "2n**64n-1n", "2n**64n+1n", "-(2n**63n)-1n", "2n**128n+5n"];
for (const C of bigTypes) {
  for (const v of bigExtremes) {
    for (const op of ops2) {
      add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));a[0]=${v};return [Atomics.${op}(a,0,1n),a[0]]})()`);
      add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));a[0]=3n;return [Atomics.${op}(a,0,${v}),a[0]]})()`);
    }
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));return [Atomics.store(a,0,${v}),a[0]]})()`);
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));return [Atomics.compareExchange(a,0,0n,${v}),a[0]]})()`);
  }
}

// ---- coerção de valor
const coerceValues = ["undefined", "null", "true", "false", "NaN", "Infinity", "-Infinity", "-0", "0.9", "-0.9", "1.5", "'7'", "'0x10'", "' 8 '", "'abc'", "''", "[]", "[5]", "({})", "({valueOf(){return 6}})", "({valueOf(){throw new RangeError('boom')}})", "Symbol()", "1n", "2**53", "-(2**31)-0.5", "1e21"];
for (const C of intTypes) {
  for (const v of coerceValues) {
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));a[0]=10;return [Atomics.add(a,0,${v}),a[0]]})()`);
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(${4 * sizeOf[C]}));return [Atomics.store(a,0,${v}),a[0]]})()`);
  }
}
for (const v of coerceValues) {
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));a[0]=0;return [Atomics.compareExchange(a,0,${v},9),a[0]]})()`);
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));a[0]=0;return [Atomics.exchange(a,0,${v}),a[0]]})()`);
}
const bigCoerce = ["undefined", "null", "1", "1.5", "true", "false", "'5'", "'abc'", "''", "' 9 '", "'0x10'", "'1.5'", "[]", "[7]", "({valueOf(){return 4n}})", "Symbol()", "NaN", "Infinity", "-0", "2n**70n"];
for (const C of bigTypes) {
  for (const v of bigCoerce) {
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));return [Atomics.store(a,0,${v}),a[0]]})()`);
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));a[0]=1n;return [Atomics.add(a,0,${v}),a[0]]})()`);
    add(`(function(){var a=new ${C}(new SharedArrayBuffer(32));a[0]=1n;return [Atomics.compareExchange(a,0,1n,${v}),a[0]]})()`);
  }
}
// store devolve o valor coerçado (ToIntegerOrInfinity), não o gravado
add(`Object.is(Atomics.store(new Int32Array(4), 0, -0), 0)`);
add(`Object.is(Atomics.store(new Int32Array(4), 0, -0.5), 0)`);
add(`Object.is(Atomics.store(new Int32Array(new SharedArrayBuffer(16)), 0, -0), 0)`);
add(`Atomics.store(new Int8Array(new SharedArrayBuffer(4)), 0, 300)`);
add(`Atomics.store(new Int8Array(new SharedArrayBuffer(4)), 0, Infinity)`);
add(`Atomics.store(new Int8Array(new SharedArrayBuffer(4)), 0, -Infinity)`);
add(`Atomics.store(new Int8Array(new SharedArrayBuffer(4)), 0, 1e300)`);
add(`Atomics.store(new Int8Array(new SharedArrayBuffer(4)), 0, 3.99)`);
add(`Atomics.store(new Int8Array(new SharedArrayBuffer(4)), 0, '12abc')`);
add(`(function(){var a=new Int8Array(new SharedArrayBuffer(4));Atomics.store(a,0,Infinity);return a[0]})()`);
add(`(function(){var a=new Uint8Array(new SharedArrayBuffer(4));Atomics.store(a,0,1e300);return a[0]})()`);
add(`Atomics.store(new BigInt64Array(new SharedArrayBuffer(8)), 0, 2n**64n)`);
add(`Atomics.store(new BigUint64Array(new SharedArrayBuffer(8)), 0, -1n)`);

// ---- tipos que não servem
for (const kind of ["shared", "plain"]) {
  for (const C of badTypes) {
    for (const op of ["add", "and", "compareExchange", "exchange", "load", "or", "store", "sub", "xor"]) {
      add(`Atomics.${op}(${mk(C, kind)},0,1,1)`);
    }
    for (const op of ["wait", "waitAsync", "notify"]) add(`Atomics.${op}(${mk(C, kind)},0,0,0)`);
  }
  for (const C of intTypes.filter((c) => c !== "Int32Array")) {
    for (const op of ["wait", "waitAsync", "notify"]) add(`Atomics.${op}(${mk(C, kind)},0,0,0)`);
  }
}
const notTyped = ["undefined", "null", "0", "'abc'", "({})", "[]", "[1,2]", "new DataView(new ArrayBuffer(8))", "new ArrayBuffer(8)", "new SharedArrayBuffer(8)", "Symbol()", "1n", "function(){}", "Object.create(Int32Array.prototype)", "{length:4}"];
for (const v of notTyped) {
  for (const op of ["add", "load", "store", "compareExchange", "exchange", "isLockFree", "notify", "wait", "waitAsync", "sub", "and", "or", "xor"]) {
    if (op === "isLockFree") continue;
    add(`Atomics.${op}(${v},0,0,0)`);
  }
}
add(`Atomics.add()`);
add(`Atomics.load()`);
add(`Atomics.store()`);
add(`Atomics.wait()`);
add(`Atomics.waitAsync()`);
add(`Atomics.notify()`);
add(`Atomics.compareExchange()`);
add(`Atomics.add(new Int32Array(1))`);
add(`Atomics.load(new Int32Array(1))`);
add(`Atomics.store(new Int32Array(1))`);
add(`Atomics.store(new Int32Array(1), 0)`);
add(`Atomics.compareExchange(new Int32Array(1), 0)`);
add(`Atomics.compareExchange(new Int32Array(1), 0, 1)`);
add(`Atomics.add(new Int32Array(1), 0)`);
add(`[1,2].map(Atomics.isLockFree)`);

// ---- índices
const indices = ["-1", "-0", "0", "1", "3", "4", "5", "2**32", "2**53", "2**53-1", "2**53+1", "Infinity", "-Infinity", "NaN", "undefined", "null", "true", "false", "0.5", "1.9", "-0.5", "-1.5", "'0'", "'1'", "'2'", "'1.5'", "'abc'", "''", "' 1 '", "'0x1'", "'-1'", "[]", "[1]", "[2,3]", "({})", "({valueOf(){return 2}})", "({valueOf(){throw new SyntaxError('idx')}})", "Symbol()", "1n", "2**31", "2**31-1", "4294967295", "4294967296", "1e300"];
for (const kind of ["shared", "plain"]) {
  for (const C of ["Int8Array", "Int32Array", "BigInt64Array"]) {
    for (const idx of indices) {
      const v = lit(C, 1);
      add(`(function(){var a=${mk(C, kind)};return Atomics.load(a,${idx})})()`);
      add(`(function(){var a=${mk(C, kind)};return [Atomics.add(a,${idx},${v}),Array.from(a)]})()`);
      add(`(function(){var a=${mk(C, kind)};return [Atomics.store(a,${idx},${v}),Array.from(a)]})()`);
      add(`(function(){var a=${mk(C, kind)};return [Atomics.compareExchange(a,${idx},${lit(C, 0)},${v}),Array.from(a)]})()`);
    }
  }
}
// ordem de avaliação: índice antes do valor, tipo antes do índice
add(`(function(){var log=[];try{Atomics.add(new Int32Array(4),{valueOf(){log.push('i');return 9}},{valueOf(){log.push('v');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.add(new Int32Array(4),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('v');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.add(new Float64Array(4),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('v');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.compareExchange(new Int32Array(4),0,{valueOf(){log.push('e');return 0}},{valueOf(){log.push('r');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.store(new Int32Array(4),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('v');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var a=new Int32Array(4);var r=Atomics.add(a,{valueOf(){return 1}},{valueOf(){return 5}});return [r,Array.from(a)]})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;return [Atomics.add(a,0,{valueOf(){structuredClone(d,{transfer:[d]});return 1}}),d.byteLength]})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;return Atomics.store(a,0,{valueOf(){structuredClone(d,{transfer:[d]});return 1}})})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;d.transfer();return Atomics.load(a,0)})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;d.transfer();return Atomics.add(a,0,1)})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;d.transfer();return Atomics.store(a,0,1)})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;d.transfer();return Atomics.compareExchange(a,0,0,1)})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;d.transfer();return Atomics.wait(a,0,0,0)})()`);
add(`(function(){var a=new Int32Array(4);var d=a.buffer;d.transfer();return Atomics.notify(a,0)})()`);

// ---- visões: byteOffset, subarray, length-tracking, growable
add(`(function(){var b=new SharedArrayBuffer(16);var a=new Int32Array(b,4,2);return [Atomics.add(a,0,5),Atomics.add(a,1,6),Array.from(new Int32Array(b)),(function(){try{return Atomics.load(a,2)}catch(e){return e.name+': '+e.message}})()]})()`);
add(`(function(){var b=new SharedArrayBuffer(16);var a=new Int32Array(b).subarray(2);return [Atomics.add(a,0,5),Atomics.load(a,1),(function(){try{return Atomics.load(a,2)}catch(e){return e.name}})()]})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);var r=[a.length];b.grow(16);r.push(a.length,Atomics.add(a,3,2),Atomics.load(a,3));return r})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);try{Atomics.load(a,3)}catch(e){var m=e.name+': '+e.message}b.grow(16);return [m,Atomics.load(a,3)]})()`);
add(`(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b);b.resize(16);return [a.length,Atomics.add(a,3,2),Atomics.load(a,3)]})()`);
add(`(function(){var b=new ArrayBuffer(16,{maxByteLength:16});var a=new Int32Array(b);b.resize(4);try{return Atomics.load(a,2)}catch(e){return e.name+': '+e.message}})()`);
add(`(function(){var b=new ArrayBuffer(16,{maxByteLength:16});var a=new Int32Array(b,8,2);b.resize(8);try{return Atomics.load(a,0)}catch(e){return e.name+': '+e.message}})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});var a=new Int32Array(b,0,2);b.grow(16);return [a.length,Atomics.load(a,1)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(8));Atomics.store(a,0,0x12345678);return Array.from(new Uint8Array(a.buffer))})()`);
add(`(function(){var b=new SharedArrayBuffer(8);var a=new Int32Array(b),c=new Uint8Array(b),d=new Int16Array(b);Atomics.store(a,0,-2);return [c[0],c[1],c[3],d[0],d[1],Atomics.load(c,0),Atomics.load(d,1)]})()`);
add(`(function(){var b=new SharedArrayBuffer(16);var a=new BigInt64Array(b),c=new Int32Array(b);Atomics.store(a,0,-1n);return [c[0],c[1],Atomics.add(a,0,2n),Atomics.load(a,0)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(8));a[1]=-1;return [Atomics.and(a,1,0xf0),Atomics.or(a,1,1),Atomics.xor(a,1,0xff),Atomics.load(a,1)]})()`);

// ---- isLockFree
for (const n of [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, -1, 16, 32, 64, 4.9, 8.9, "4", "8", "1", "'abc'", "undefined", "null", "true", "NaN", "Infinity", "2**32+4", "2**32+8", "-4294967292", "[4]", "({valueOf(){return 8}})", "({valueOf(){throw new RangeError('lf')}})", "Symbol()", "4n", "0.5", "1.5", "3.99"]) {
  add(`Atomics.isLockFree(${n})`);
}
add(`Atomics.isLockFree()`);
add(`Atomics.isLockFree(4, 8)`);
add(`typeof Atomics.isLockFree(4)`);

// ---- wait: só os caminhos que não bloqueiam
const waitValues = ["0", "1", "-1", "0.5", "'0'", "'1'", "undefined", "null", "true", "NaN", "2**32", "2**32+1", "-(2**32)", "({valueOf(){return 0}})", "Symbol()", "1n"];
const timeouts = ["0", "-1", "-Infinity", "-0", "NaN", "1", "'0'", "undefined"];
for (const v of waitValues) {
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));return Atomics.wait(a,0,${v},0)})()`);
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));return Atomics.waitAsync(a,0,${v},0)})()`);
}
for (const t of ["0", "-1", "-Infinity", "-0", "1", "'0'", "({valueOf(){return 0}})", "Symbol()", "1n", "null", "false"]) {
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));return Atomics.wait(a,0,0,${t})})()`);
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));return Atomics.wait(a,0,1,${t})})()`);
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));return Atomics.waitAsync(a,0,0,${t})})()`);
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));return Atomics.waitAsync(a,0,1,${t})})()`);
  add(`(function(){var a=new BigInt64Array(new SharedArrayBuffer(16));return Atomics.wait(a,0,0n,${t})})()`);
  add(`(function(){var a=new BigInt64Array(new SharedArrayBuffer(16));return Atomics.waitAsync(a,0,1n,${t})})()`);
}
for (const v of ["0n", "1n", "-1n", "2n**64n", "2n**64n+1n", "2n**63n", "'1'", "'0'", "1", "0", "undefined", "true", "({valueOf(){return 0n}})", "Symbol()"]) {
  add(`(function(){var a=new BigInt64Array(new SharedArrayBuffer(16));return Atomics.wait(a,0,${v},0)})()`);
  add(`(function(){var a=new BigInt64Array(new SharedArrayBuffer(16));return Atomics.waitAsync(a,0,${v},0)})()`);
}
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));a[1]=7;return [Atomics.wait(a,1,7,0),Atomics.wait(a,1,8,0),Atomics.wait(a,0,7,0)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));a[1]=7;return [Atomics.wait(a,1,7,1),Atomics.wait(a,1,8,1)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));a[1]=7;var r=Atomics.waitAsync(a,1,8,0);return [r.async,r.value,Object.keys(r),Object.getPrototypeOf(r)===Object.prototype]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,0);return [r.async,r.value]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,10);return [r.async,r.value instanceof Promise,Atomics.notify(a,0),Atomics.notify(a,0)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0);return [r.async,r.value instanceof Promise,Atomics.notify(a,0),Atomics.notify(a,0)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0,Infinity);return [r.async,Atomics.notify(a,1),Atomics.notify(a,0,0),Atomics.notify(a,0,1)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0);Atomics.waitAsync(a,0,0);Atomics.waitAsync(a,0,0);return [Atomics.notify(a,0,2),Atomics.notify(a,0),Atomics.notify(a,0)]})()`);
add(`(function(){var a=new BigInt64Array(new SharedArrayBuffer(16));var r=Atomics.waitAsync(a,0,0n);return [r.async,Atomics.notify(a,0)]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));a[0]=1;var r=Atomics.waitAsync(a,0,1,undefined);return [r.async,Atomics.notify(a,0,undefined)]})()`);
for (const idx of ["-1", "4", "1.5", "'1'", "NaN", "undefined", "Infinity", "({})", "Symbol()"]) {
  add(`Atomics.wait(new Int32Array(new SharedArrayBuffer(16)),${idx},0,0)`);
  add(`Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),${idx},0,0)`);
  add(`Atomics.notify(new Int32Array(new SharedArrayBuffer(16)),${idx},1)`);
  add(`Atomics.notify(new Int32Array(16),${idx},1)`);
}

// ---- notify: variantes de count e buffer não compartilhado
for (const cnt of ["undefined", "0", "-1", "1", "1.9", "-0", "Infinity", "-Infinity", "NaN", "'2'", "'abc'", "null", "true", "2**32", "2**53", "[3]", "({valueOf(){return 1}})", "({valueOf(){throw new RangeError('cnt')}})", "Symbol()", "1n"]) {
  add(`Atomics.notify(new Int32Array(new SharedArrayBuffer(16)),0,${cnt})`);
  add(`Atomics.notify(new Int32Array(16),0,${cnt})`);
  add(`Atomics.notify(new BigInt64Array(new SharedArrayBuffer(16)),0,${cnt})`);
  add(`(function(){var a=new Int32Array(new SharedArrayBuffer(16));Atomics.waitAsync(a,0,0);Atomics.waitAsync(a,0,0);return Atomics.notify(a,0,${cnt})})()`);
}
add(`Atomics.notify(new Int32Array(new SharedArrayBuffer(16)),0)`);
add(`Atomics.notify(new Int32Array(16),0)`);
add(`Atomics.notify(new Int32Array(16),4)`);
add(`Atomics.notify(new Int32Array(16),-1)`);
add(`Atomics.notify(new Int32Array(16),'x')`);
add(`(function(){var log=[];try{Atomics.notify(new Int32Array(new SharedArrayBuffer(16)),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('c');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.notify(new Int32Array(16),{valueOf(){log.push('i');return 99}},{valueOf(){log.push('c');return 1}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.wait(new Int32Array(new SharedArrayBuffer(16)),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('v');return 0}},{valueOf(){log.push('t');return 0}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.wait(new Int32Array(16),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('v');return 0}},{valueOf(){log.push('t');return 0}})}catch(e){log.push(e.name)}return log})()`);
add(`(function(){var log=[];try{Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(16)),{valueOf(){log.push('i');return 0}},{valueOf(){log.push('v');return 5}},{valueOf(){log.push('t');return 0}})}catch(e){log.push(e.name)}return log})()`);

// ---- pause
for (const v of ["", "undefined", "0", "1", "-1", "100", "-0", "1.5", "NaN", "Infinity", "'1'", "null", "true", "1n", "({})", "[]", "Symbol()", "2**53", "2**31", "-(2**31)"]) {
  add(`Atomics.pause(${v})`);
}
add(`Atomics.pause(1,2,3)`);
add(`Atomics.pause.length`);
add(`Atomics.pause.call(null, 1)`);
add(`Atomics.pause.call(undefined)`);

// ---- this e chamadas por referência
add(`(function(){var f=Atomics.add;return f(new Int32Array(2),0,1)})()`);
add(`Atomics.add.call(null,new Int32Array(2),0,1)`);
add(`Atomics.load.call(undefined,new Int32Array(2),0)`);
add(`Atomics.add.apply(null,[new Int32Array(2),0,1])`);
add(`Reflect.apply(Atomics.add,undefined,[new Int32Array([4]),0,3])`);
add(`Atomics.add(new Int32Array([1]),0,1,2,3)`);
add(`(function(){var o=Object.create(Atomics);return o.add(new Int32Array([2]),0,1)})()`);
add(`(function(){Atomics.add=1;return typeof Atomics.add})()`);
add(`(function(){var a=Atomics.add;Atomics.add=1;var t=typeof Atomics.add;Atomics.add=a;return t})()`);
add(`(function(){var a=Atomics.add;delete Atomics.add;var r=typeof Atomics.add;Atomics.add=a;return r})()`);
add(`(function(){'use strict';try{Atomics[Symbol.toStringTag]='x'}catch(e){return e.name}return 'ok'})()`);
add(`Atomics.load(new Proxy(new Int32Array(2),{}),0)`);
add(`Atomics.load(Object.setPrototypeOf(new Int32Array([5]),null),0)`);
add(`(function(){var a=new Int32Array([5]);Object.defineProperty(a,'length',{value:0});return Atomics.load(a,0)})()`);
add(`(function(){class A extends Int32Array{};return Atomics.add(new A([1,2]),1,5)})()`);
add(`(function(){class A extends BigInt64Array{};return Atomics.add(new A([1n,2n]),1,5n)})()`);

// ---- SharedArrayBuffer
add(`typeof SharedArrayBuffer`);
add(`SharedArrayBuffer.length`);
add(`SharedArrayBuffer.name`);
add(`SharedArrayBuffer.prototype[Symbol.toStringTag]`);
add(`Object.prototype.toString.call(new SharedArrayBuffer(1))`);
add(`Object.prototype.toString.call(SharedArrayBuffer.prototype)`);
add(`Object.getOwnPropertyNames(SharedArrayBuffer.prototype).sort()`);
add(`Object.getOwnPropertyNames(SharedArrayBuffer).sort()`);
add(`Object.getOwnPropertySymbols(SharedArrayBuffer).map(String)`);
add(`Object.getOwnPropertySymbols(SharedArrayBuffer.prototype).map(String)`);
add(`Object.getPrototypeOf(SharedArrayBuffer) === Function.prototype`);
add(`Object.getPrototypeOf(SharedArrayBuffer.prototype) === Object.prototype`);
add(`SharedArrayBuffer[Symbol.species] === SharedArrayBuffer`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer, Symbol.species).get.name`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'byteLength').get.name`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'growable').get.name`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'maxByteLength').get.name`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'byteLength').set`);
add(`SharedArrayBuffer.prototype.slice.length`);
add(`SharedArrayBuffer.prototype.grow.length`);
add(`SharedArrayBuffer.prototype.constructor === SharedArrayBuffer`);
add(`SharedArrayBuffer()`);
add(`SharedArrayBuffer(1)`);
add(`new SharedArrayBuffer()`.concat(".byteLength"));
add(`new SharedArrayBuffer(undefined).byteLength`);
add(`new SharedArrayBuffer(null).byteLength`);
add(`new SharedArrayBuffer(true).byteLength`);
add(`new SharedArrayBuffer('3').byteLength`);
add(`new SharedArrayBuffer(2.9).byteLength`);
add(`new SharedArrayBuffer(-0).byteLength`);
add(`new SharedArrayBuffer(NaN).byteLength`);
add(`new SharedArrayBuffer(-1)`);
add(`new SharedArrayBuffer(-0.5).byteLength`);
add(`new SharedArrayBuffer(Infinity)`);
add(`new SharedArrayBuffer(2**53)`);
add(`new SharedArrayBuffer(2**60)`);
add(`new SharedArrayBuffer('abc').byteLength`);
add(`new SharedArrayBuffer(Symbol())`);
add(`new SharedArrayBuffer(1n)`);
add(`new SharedArrayBuffer({valueOf(){return 5}}).byteLength`);
add(`new SharedArrayBuffer(8, undefined).growable`);
add(`new SharedArrayBuffer(8, null).growable`);
add(`new SharedArrayBuffer(8, {}).growable`);
add(`new SharedArrayBuffer(8, 5).growable`);
add(`new SharedArrayBuffer(8, 'x').growable`);
add(`new SharedArrayBuffer(8, {maxByteLength: undefined}).growable`);
add(`new SharedArrayBuffer(8, {maxByteLength: 8}).growable`);
add(`new SharedArrayBuffer(8, {maxByteLength: 16}).growable`);
add(`new SharedArrayBuffer(8, {maxByteLength: 4})`);
add(`new SharedArrayBuffer(8, {maxByteLength: -1})`);
add(`new SharedArrayBuffer(8, {maxByteLength: NaN})`);
add(`new SharedArrayBuffer(8, {maxByteLength: '16'}).maxByteLength`);
add(`new SharedArrayBuffer(8, {maxByteLength: 2**60})`);
add(`new SharedArrayBuffer(8, {maxByteLength: 16.9}).maxByteLength`);
add(`new SharedArrayBuffer(0, {maxByteLength: 0}).growable`);
add(`new SharedArrayBuffer(8).maxByteLength`);
add(`new SharedArrayBuffer(8).growable`);
add(`new SharedArrayBuffer(8, {maxByteLength: 16}).maxByteLength`);
add(`new SharedArrayBuffer(8, {maxByteLength: 16}).byteLength`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return [b.grow(12),b.byteLength,b.grow(12),b.byteLength,b.grow(16),b.byteLength]})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow(4)})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow(17)})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow(-1)})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow()})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow('12')})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(10.9);return b.byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);return b.grow(8)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);return b.grow(9)})()`);
add(`SharedArrayBuffer.prototype.grow.call(new ArrayBuffer(8,{maxByteLength:16}),12)`);
add(`SharedArrayBuffer.prototype.grow.call({},12)`);
add(`SharedArrayBuffer.prototype.slice.call(new ArrayBuffer(8),0)`);
add(`SharedArrayBuffer.prototype.slice.call({},0)`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'byteLength').get.call(new ArrayBuffer(8))`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'byteLength').get.call({})`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'growable').get.call(new ArrayBuffer(8))`);
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype,'maxByteLength').get.call(new ArrayBuffer(8))`);
add(`Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get.call(new SharedArrayBuffer(8))`);
add(`ArrayBuffer.prototype.slice.call(new SharedArrayBuffer(8),0)`);
add(`ArrayBuffer.isView(new SharedArrayBuffer(8))`);
add(`new SharedArrayBuffer(8) instanceof ArrayBuffer`);
add(`new ArrayBuffer(8) instanceof SharedArrayBuffer`);
add(`Object.getPrototypeOf(new SharedArrayBuffer(8)) === SharedArrayBuffer.prototype`);
// slice
for (const [s, e] of [["", ""], ["0", ""], ["2", ""], ["2", "5"], ["-3", ""], ["-3", "-1"], ["1", "-1"], ["5", "2"], ["0", "0"], ["0", "100"], ["100", ""], ["-100", "100"], ["NaN", "NaN"], ["Infinity", ""], ["-Infinity", "Infinity"], ["'2'", "'4'"], ["undefined", "undefined"], ["null", "null"], ["1.9", "4.9"], ["-0", "-0"], ["({valueOf(){return 3}})", ""], ["Symbol()", ""]]) {
  const args = e === "" ? (s === "" ? "" : s) : `${s},${e}`;
  add(`(function(){var b=new SharedArrayBuffer(8);new Uint8Array(b).set([1,2,3,4,5,6,7,8]);var c=b.slice(${args});return [c.byteLength,Array.from(new Uint8Array(c)),c instanceof SharedArrayBuffer,c!==b]})()`);
}
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});var c=b.slice(1,4);return [c.byteLength,c.growable,c.maxByteLength]})()`);
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(12);var c=b.slice(2);return [c.byteLength,c.growable]})()`);
add(`(function(){var b=new SharedArrayBuffer(8);var c=b.slice(0);new Uint8Array(c)[0]=9;return new Uint8Array(b)[0]})()`);
// species
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor=undefined;return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor=null;return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor=1;return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={};return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:undefined};return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:null};return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:1};return b.slice(0).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(n){return new SharedArrayBuffer(n+2)}};return b.slice(0,4).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(n){return new SharedArrayBuffer(n-1)}};return b.slice(0,4).byteLength})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(n){return b}};return b.slice(0,4)===b})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(n){return new ArrayBuffer(n)}};return b.slice(0,4)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(n){return {}}};return b.slice(0,4)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(n){return 5}};return b.slice(0,4)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);var args;b.constructor={[Symbol.species]:function(n){args=[n,arguments.length,new.target===undefined];return new SharedArrayBuffer(n)}};b.slice(2,6);return args})()`);
add(`(function(){var b=new SharedArrayBuffer(8);b.constructor={[Symbol.species]:function(){throw new RangeError('sp')}};return b.slice(0)})()`);
add(`(function(){class S extends SharedArrayBuffer{};var s=new S(8);var c=s.slice(0,4);return [s instanceof S,c instanceof S,c.byteLength,Object.getPrototypeOf(c)===S.prototype]})()`);
add(`(function(){class S extends SharedArrayBuffer{static get [Symbol.species](){return SharedArrayBuffer}};var s=new S(8);var c=s.slice(0,4);return [c instanceof S,c instanceof SharedArrayBuffer]})()`);
add(`(function(){class S extends SharedArrayBuffer{};var s=new S(8,{maxByteLength:16});return [s.growable,s.maxByteLength,Object.prototype.toString.call(s)]})()`);
add(`(function(){function F(){};F.prototype=Array.prototype;var b=Reflect.construct(SharedArrayBuffer,[4],F);return [Object.getPrototypeOf(b)===Array.prototype,b.byteLength]})()`);
add(`(function(){var b=Reflect.construct(SharedArrayBuffer,[4],Object);return Object.getPrototypeOf(b)===Object.prototype})()`);
add(`(function(){var nt=function(){}.bind();Object.defineProperty(nt,'prototype',{value:null});var b=Reflect.construct(SharedArrayBuffer,[4],nt);return Object.getPrototypeOf(b)===SharedArrayBuffer.prototype})()`);
// toStringTag e identidade
add(`Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, Symbol.toStringTag)`);
add(`(function(){var b=new SharedArrayBuffer(4);Object.defineProperty(b,Symbol.toStringTag,{value:'X'});return Object.prototype.toString.call(b)})()`);
add(`Object.prototype.toString.call(Object.create(SharedArrayBuffer.prototype))`);
add(`Object.prototype.toString.call(Atomics)`);
add(`String(new SharedArrayBuffer(2))`);
add(`new SharedArrayBuffer(2) + ''`);
add(`JSON.stringify(new SharedArrayBuffer(2))`);
add(`Object.keys(new SharedArrayBuffer(2))`);
add(`Object.getOwnPropertyNames(new SharedArrayBuffer(2))`);
add(`Object.isFrozen(new SharedArrayBuffer(2))`);
add(`Object.isExtensible(new SharedArrayBuffer(2))`);
add(`Object.freeze(new SharedArrayBuffer(2)).byteLength`);
add(`(function(){var b=new SharedArrayBuffer(2);b.x=1;return Object.keys(b)})()`);
add(`(function(){var a=new SharedArrayBuffer(2);var b=new SharedArrayBuffer(2);return [a===b,Object.is(a,a),a.slice(0)===a]})()`);
// structuredClone (compartilha o bloco; sem transferir)
add(`(function(){var b=new SharedArrayBuffer(4);var c=structuredClone(b);new Uint8Array(b)[0]=7;return [c instanceof SharedArrayBuffer,c!==b,new Uint8Array(c)[0],c.byteLength]})()`);
add(`(function(){var b=new SharedArrayBuffer(4,{maxByteLength:8});var c=structuredClone(b);b.grow(8);return [c.growable,c.byteLength,c.maxByteLength]})()`);
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(8));a[0]=3;var c=structuredClone(a);Atomics.add(a,0,1);return [c instanceof Int32Array,c.buffer instanceof SharedArrayBuffer,c[0]]})()`);
add(`(function(){var b=new SharedArrayBuffer(4);return structuredClone(b,{transfer:[b]})})()`);
add(`structuredClone({a:new SharedArrayBuffer(3)}).a.byteLength`);
// Atomics com typed array em SAB repartido por clone
add(`(function(){var a=new Int32Array(new SharedArrayBuffer(8));var c=structuredClone(a);Atomics.store(a,1,42);return Atomics.load(c,1)})()`);
// ctor de typed array sobre SAB
add(`(function(){var b=new SharedArrayBuffer(8);var u=new Uint8Array(b);u.set([1,2,3]);return [Array.from(new Uint8Array(b.slice(1))),new Int16Array(b).length,new DataView(b).getUint8(2)]})()`);
add(`(function(){var b=new SharedArrayBuffer(8);return new Int32Array(b,1)})()`);
add(`(function(){var b=new SharedArrayBuffer(7);return new Int32Array(b)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);return new Int32Array(b,0,3)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);return new Int32Array(b,12)})()`);
add(`(function(){var b=new SharedArrayBuffer(8);return new BigInt64Array(b,4)})()`);
add(`(function(){var u=new Int32Array(new SharedArrayBuffer(8));return [u.buffer.byteLength,u.byteLength,u.byteOffset,Object.prototype.toString.call(u.buffer)]})()`);
add(`(function(){var u=new Uint8Array(new SharedArrayBuffer(4));u.set([3,1,2,0]);u.sort();return Array.from(u)})()`);
add(`(function(){var u=new Uint8Array(new SharedArrayBuffer(4));return [u.fill(3).join(),u.slice(1).buffer instanceof SharedArrayBuffer,u.subarray(1).buffer instanceof SharedArrayBuffer]})()`);
add(`(function(){var u=new Uint8Array(new SharedArrayBuffer(4));return u.buffer.transfer()})()`);
add(`(function(){var b=new SharedArrayBuffer(4);return ArrayBuffer.prototype.transfer.call(b)})()`);
add(`(function(){var b=new SharedArrayBuffer(4);return Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'detached').get.call(b)})()`);
add(`'detached' in SharedArrayBuffer.prototype`);
add(`'resize' in SharedArrayBuffer.prototype`);
add(`'transfer' in SharedArrayBuffer.prototype`);
add(`'isView' in SharedArrayBuffer`);

// ---- DataView: get/set de todos os tipos, endianness, Float16, limites e erros.
const dvTypes = [["Int8", 1], ["Uint8", 1], ["Int16", 2], ["Uint16", 2], ["Int32", 4], ["Uint32", 4], ["Float16", 2], ["Float32", 4], ["Float64", 8], ["BigInt64", 8], ["BigUint64", 8]];
const dvVals = {
  int: ["0", "1", "-1", "127", "128", "255", "256", "32767", "65535", "65536", "2**31", "2**32-1", "2**32+5", "1.9", "-1.9", "NaN", "Infinity", "'7'", "undefined", "null", "true"],
  float: ["0", "-0", "1.5", "-1.5", "0.1", "65504", "65520", "65519.99", "1e-8", "6e-8", "2**-24", "2**-25", "3.4028235e38", "3.5e38", "1e39", "NaN", "Infinity", "-Infinity", "5e-324", "1.00048828125", "1.0009765625"],
  big: ["0n", "1n", "-1n", "2n**63n", "2n**64n-1n", "2n**64n", "-(2n**63n)", "-(2n**63n)-1n", "2n**64n+7n"],
};
for (const [name, size] of dvTypes) {
  const isBig = name.startsWith("Big"), isFloat = name.startsWith("Float");
  const set = `set${name}`, get = `get${name}`;
  const list = isBig ? dvVals.big : isFloat ? dvVals.float : dvVals.int;
  for (const v of list) {
    for (const le of ["", ",true", ",false"]) {
      add(`(function(){var d=new DataView(new ArrayBuffer(16));d.${set}(2,${v}${le});return [d.${get}(2${le}),Array.from(new Uint8Array(d.buffer,2,${size})).join(),d.${get}(2${le === ",true" ? ",false" : ",true"})]})()`);
    }
  }
  if (!isBig) add(`(function(){var d=new DataView(new ArrayBuffer(16));d.${set}(0,${isFloat ? "1.5" : "1"},true);return Object.is(d.${get}(0,true),-0)})()`);
  add(`(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}(0,${isBig ? "1n" : "1"})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(${16 - size})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(${17 - size})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(-1)})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(2**53)})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(Infinity)})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}("1")})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(1.9)})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(NaN)})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}(Symbol())})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${get}()})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}()})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}(${17 - size},${isBig ? "1n" : "1"})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}(-1,${isBig ? "1n" : "1"})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}(0,${isBig ? "1" : "1n"})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}(0,Symbol())})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));return d.${set}(0,{valueOf(){throw new RangeError("v")}})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16),4,${size});return [d.${get}(0),d.${get}(${size - 1 === 0 ? 1 : 0})]})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16),4,${size});return d.${get}(1)})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16),4);return d.${get}(${12 - size})})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16),4);return d.${get}(${13 - size})})()`,
    `(function(){var d=new DataView(new SharedArrayBuffer(16));d.${set}(1,${isBig ? "3n" : "3"},true);return d.${get}(1,true)})()`,
    `(function(){return DataView.prototype.${get}.call({},0)})()`,
    `(function(){return DataView.prototype.${get}.call(new Uint8Array(16),0)})()`,
    `(function(){return DataView.prototype.${set}.call(new ArrayBuffer(16),0,1)})()`,
    `(function(){return [DataView.prototype.${get}.length,DataView.prototype.${set}.length,DataView.prototype.${get}.name,DataView.prototype.${set}.name]})()`,
    `(function(){var d=new DataView(new ArrayBuffer(16));var o=[];d.${set}({valueOf(){o.push("i");return 1}},{valueOf(){o.push("v");return ${isBig ? "1n" : "1"}}},{valueOf(){o.push("e");return true}});return o})()`,
    `(function(){var b=new ArrayBuffer(16);var d=new DataView(b);var o=[];try{d.${set}({valueOf(){o.push("i");return 100}},{valueOf(){o.push("v");return ${isBig ? "1n" : "1"}}})}catch(e){o.push(e.name)}return o})()`,
    `(function(){var b=new ArrayBuffer(16);var d=new DataView(b);b.transfer();return d.${get}(0)})()`,
    `(function(){var b=new ArrayBuffer(16);var d=new DataView(b);b.transfer();return d.${set}(0,${isBig ? "1n" : "1"})})()`,
    `(function(){var b=new ArrayBuffer(16);var d=new DataView(b);return d.${set}({valueOf(){b.transfer();return 0}},${isBig ? "1n" : "1"})})()`,
    `(function(){var b=new ArrayBuffer(16);var d=new DataView(b);return d.${set}(0,{valueOf(){b.transfer();return ${isBig ? "1n" : "1"}}})})()`);
}
// Padrões de bytes conhecidos lidos em todas as larguras, nas duas endianness.
for (const bytes of ["[1,2,3,4,5,6,7,8]", "[255,255,255,255,255,255,255,255]", "[128,0,0,0,0,0,0,0]", "[0,0,0,0,0,0,0,128]", "[0x3c,0,0x7c,0,0xfc,0,0x7e,0]"]) {
  for (const [name] of dvTypes) {
    add(`(function(){var d=new DataView(new Uint8Array(${bytes}).buffer);return [d.get${name}(0),d.get${name}(0,true)]})()`);
  }
}
// Construtor do DataView.
for (const args of ["", "undefined", "null", "{}", "[]", "1", "'a'", "new Uint8Array(8)", "new DataView(new ArrayBuffer(8))", "new ArrayBuffer(8)", "new ArrayBuffer(8),0", "new ArrayBuffer(8),8", "new ArrayBuffer(8),9", "new ArrayBuffer(8),-1", "new ArrayBuffer(8),1.5", "new ArrayBuffer(8),'2'", "new ArrayBuffer(8),NaN", "new ArrayBuffer(8),Infinity", "new ArrayBuffer(8),Symbol()", "new ArrayBuffer(8),0,8", "new ArrayBuffer(8),0,9", "new ArrayBuffer(8),4,4", "new ArrayBuffer(8),4,5", "new ArrayBuffer(8),0,-1", "new ArrayBuffer(8),0,undefined", "new ArrayBuffer(8),undefined,2", "new ArrayBuffer(8),0,null", "new ArrayBuffer(8),0,1.9", "new ArrayBuffer(8),0,Infinity", "new ArrayBuffer(8),8,0", "new ArrayBuffer(8),9,0", "new SharedArrayBuffer(8),2", "new SharedArrayBuffer(8),2,3", "new ArrayBuffer(8,{maxByteLength:16}),2", "new ArrayBuffer(8,{maxByteLength:16}),2,3", "new ArrayBuffer(8,{maxByteLength:16}),9", "new SharedArrayBuffer(8,{maxByteLength:16}),2", "new SharedArrayBuffer(8,{maxByteLength:16}),2,3"]) {
  add(`(function(){var d=new DataView(${args});return [d.byteLength,d.byteOffset,d.buffer.byteLength,Object.prototype.toString.call(d)]})()`);
}
add(`DataView()`, `DataView.length`, `DataView.name`, `Object.prototype.toString.call(DataView.prototype)`, `Object.getPrototypeOf(DataView)===Function.prototype`,
  `Object.getOwnPropertyNames(DataView.prototype).sort().join()`, `DataView.prototype.constructor===DataView`, `DataView.prototype[Symbol.toStringTag]`,
  `(function(){return DataView.prototype.byteLength})()`, `(function(){return DataView.prototype.byteOffset})()`, `(function(){return DataView.prototype.buffer})()`,
  `(function(){return Object.getOwnPropertyDescriptor(DataView.prototype,"byteLength").get.call({})})()`,
  `(function(){return Object.getOwnPropertyDescriptor(DataView.prototype,"buffer").get.call(new Uint8Array(1))})()`,
  `(function(){return Object.getOwnPropertyDescriptor(DataView.prototype,"byteOffset").get.name})()`,
  `(function(){class D extends DataView{};var d=new D(new ArrayBuffer(8),1);return [d instanceof DataView,d.byteLength,Object.getPrototypeOf(d)===D.prototype]})()`,
  `(function(){function N(){};N.prototype=null;var d=Reflect.construct(DataView,[new ArrayBuffer(8)],N);return [Object.getPrototypeOf(d)===DataView.prototype,d.byteLength]})()`,
  `(function(){var nt=function(){}.bind();return Reflect.construct(DataView,[new ArrayBuffer(8)],nt).byteLength})()`,
  `(function(){var b=new ArrayBuffer(8);var d=new DataView(b,2,4);b.transfer();return [d.byteLength]})()`,
  `(function(){var b=new ArrayBuffer(8);var d=new DataView(b,2,4);b.transfer();return d.byteOffset})()`,
  `(function(){var b=new ArrayBuffer(8);var d=new DataView(b,2,4);b.transfer();return d.buffer===b})()`,
  `(function(){var b=new ArrayBuffer(8);var d=new DataView(b,2);b.transfer();return d.byteLength})()`,
  `(function(){var b=new ArrayBuffer(8);b.transfer();return new DataView(b)})()`);
// DataView sobre ArrayBuffer redimensionável: length-tracking e tamanho fixo.
for (const [label, make] of [["tracking", "new DataView(b)"], ["trackingOff", "new DataView(b,2)"], ["fixed", "new DataView(b,2,4)"], ["fixedAll", "new DataView(b,0,8)"]]) {
  const pre = `var b=new ArrayBuffer(8,{maxByteLength:16});var d=${make};`;
  for (const size of [0, 1, 2, 3, 4, 6, 7, 8, 9, 12, 16]) {
    add(`(function(){${pre}b.resize(${size});var r=[];try{r.push(d.byteLength)}catch(e){r.push(e.name+": "+e.message)}try{r.push(d.byteOffset)}catch(e){r.push(e.name+": "+e.message)}try{r.push(d.getUint8(0))}catch(e){r.push(e.name+": "+e.message)}return r})()`);
  }
  add(`(function(){${pre}b.resize(16);d.setUint8(15,9);return [d.byteLength,d.getUint8(15)]})()`,
    `(function(){${pre}b.resize(4);b.resize(8);return [d.byteLength,d.byteOffset,d.getUint8(3)]})()`,
    `(function(){${pre}b.transfer();return [d.buffer===b,b.detached]})()`,
    `(function(){${pre}b.transfer();return d.byteLength})()`,
    `(function(){${pre}b.transfer();return d.byteOffset})()`);
}
// ---- ArrayBuffer: redimensionável, transfer, transferToFixedLength, detached.
const abInit = ["0", "1", "8", "16"];
for (const len of abInit) {
  for (const max of ["undefined", "0", "16", "32", "64"]) {
    const opts = max === "undefined" ? "" : `,{maxByteLength:${max}}`;
    add(`(function(){var b=new ArrayBuffer(${len}${opts});return [b.byteLength,b.maxByteLength,b.resizable,b.detached]})()`);
  }
}
for (const n of ["0", "1", "7", "8", "9", "16", "17", "32", "33", "-1", "1.9", "NaN", "'4'", "undefined", "null", "Infinity", "2**53", "Symbol()", "1n", "{valueOf(){return 12}}"]) {
  add(`(function(){var b=new ArrayBuffer(8,{maxByteLength:32});b.resize(${n});return [b.byteLength,b.resizable]})()`,
    `(function(){var b=new ArrayBuffer(8);b.resize(${n});return b.byteLength})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:32});b.transfer();return b.resize(${n})})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:32});var t=b.transfer(${n});return [t.byteLength,t.maxByteLength,t.resizable,b.byteLength,b.detached]})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:32});var t=b.transferToFixedLength(${n});return [t.byteLength,t.maxByteLength,t.resizable,b.byteLength,b.detached]})()`,
    `(function(){var b=new ArrayBuffer(8);var t=b.transfer(${n});return [t.byteLength,t.maxByteLength,t.resizable,b.detached]})()`,
    `(function(){var b=new ArrayBuffer(8);var t=b.transferToFixedLength(${n});return [t.byteLength,t.resizable,b.detached]})()`);
}
add(`(function(){var b=new ArrayBuffer(4);new Uint8Array(b).set([1,2,3,4]);var t=b.transfer(8);return [Array.from(new Uint8Array(t)),b.byteLength,b.detached]})()`,
  `(function(){var b=new ArrayBuffer(4);new Uint8Array(b).set([1,2,3,4]);var t=b.transfer(2);return Array.from(new Uint8Array(t))})()`,
  `(function(){var b=new ArrayBuffer(4,{maxByteLength:8});new Uint8Array(b).set([1,2,3,4]);var t=b.transfer();return [Array.from(new Uint8Array(t)),t.resizable,t.maxByteLength]})()`,
  `(function(){var b=new ArrayBuffer(4,{maxByteLength:8});var t=b.transfer(16);return [t.byteLength,t.maxByteLength]})()`,
  `(function(){var b=new ArrayBuffer(4,{maxByteLength:8});var t=b.transfer(8);return [t.byteLength,t.maxByteLength]})()`,
  `(function(){var b=new ArrayBuffer(4,{maxByteLength:8});b.transfer();return b.maxByteLength})()`,
  `(function(){var b=new ArrayBuffer(4,{maxByteLength:8});b.transfer();return b.resizable})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return [b.byteLength,b.maxByteLength,b.resizable,b.detached]})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return b.transfer()})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return b.transferToFixedLength()})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return b.slice(0)})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return b.resize(0)})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return new Uint8Array(b)})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return new DataView(b)})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return structuredClone(b)})()`,
  `(function(){var b=new ArrayBuffer(4);b.transfer();return ArrayBuffer.prototype.slice.call(b,0,1)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return [u.length,u.byteLength,u.byteOffset,u[0],u.buffer===b]})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.fill(1)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return Array.from(u)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();u[0]=5;return [u[0],Object.keys(u).length,0 in u]})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return Object.getOwnPropertyNames(u).join()})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.at(0)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.subarray(0)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.slice()})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.join()})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.set([1])})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return new Uint8Array(u)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return u.entries().next()})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);var it=u.values();it.next();b.transfer();return it.next()})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Uint8Array(b);b.transfer();return Atomics.load(new Int8Array(b),0)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Int32Array(b);b.transfer();return Atomics.add(u,0,1)})()`,
  `(function(){var b=new ArrayBuffer(4);var u=new Int32Array(b);b.transfer();return Atomics.notify(u,0)})()`,
  `(function(){var b=new ArrayBuffer(4);return [Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"detached").get.call(b),ArrayBuffer.prototype.transfer.length,ArrayBuffer.prototype.transferToFixedLength.length,ArrayBuffer.prototype.resize.length]})()`,
  `ArrayBuffer.prototype.transfer.call({})`, `ArrayBuffer.prototype.transfer.call(new SharedArrayBuffer(1))`, `ArrayBuffer.prototype.transferToFixedLength.call(new Uint8Array(1))`,
  `ArrayBuffer.prototype.resize.call(new SharedArrayBuffer(1,{maxByteLength:2}),1)`, `ArrayBuffer.prototype.resize.call({},1)`,
  `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"detached").get.call({})`, `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"resizable").get.call({})`,
  `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"maxByteLength").get.call(new SharedArrayBuffer(1))`, `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"byteLength").get.call(new SharedArrayBuffer(1))`,
  `new ArrayBuffer(8,{maxByteLength:4})`, `new ArrayBuffer(8,{maxByteLength:-1})`, `new ArrayBuffer(8,{maxByteLength:2**53})`, `new ArrayBuffer(8,{maxByteLength:undefined}).resizable`, `new ArrayBuffer(8,{maxByteLength:null}).resizable`,
  `new ArrayBuffer(8,{maxByteLength:'16'}).maxByteLength`, `new ArrayBuffer(8,{maxByteLength:8.9}).maxByteLength`, `new ArrayBuffer(8,{maxByteLength:NaN}).resizable`, `new ArrayBuffer(8,8).resizable`, `new ArrayBuffer(8,null).resizable`,
  `(function(){var b=new ArrayBuffer(4);new Uint8Array(b).set([1,2,3,4]);return [Array.from(new Uint8Array(b.slice(1,3))),b.slice(-2).byteLength,b.slice(5).byteLength]})()`,
  `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var s=b.slice(2);return [s.byteLength,s.resizable]})()`,
  `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});return ArrayBuffer.isView(new Uint8Array(b))})()`,
  `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});return Object.prototype.toString.call(b)+b.constructor.name})()`,
  `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});return structuredClone(b).resizable})()`,
  `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var c=structuredClone(b);return [c.byteLength,c.maxByteLength]})()`,
  `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var c=structuredClone(b,{transfer:[b]});return [c.byteLength,c.maxByteLength,b.detached]})()`,
  `(function(){var b=new ArrayBuffer(8);var c=structuredClone(b,{transfer:[b]});return [c.byteLength,b.byteLength,b.detached]})()`,
  `(function(){var b=new ArrayBuffer(8);return structuredClone(b,{transfer:[b,b]})})()`,
  `(function(){var b=new ArrayBuffer(8);b.transfer();return structuredClone(b,{transfer:[b]})})()`);
// Typed arrays sobre ArrayBuffer redimensionável (length-tracking e fixo) em todos os tipos.
for (const t of [...allTypes, "Uint8ClampedArray", "Float16Array", "Float32Array", "Float64Array"]) {
  const sz = sizeOf[t], zero = t.startsWith("Big") ? "0n" : "0";
  const pre = `var b=new ArrayBuffer(${sz * 4},{maxByteLength:${sz * 8}});`;
  add(`(function(){${pre}var u=new ${t}(b);var r=[u.length];b.resize(${sz * 6});r.push(u.length,u.byteLength);b.resize(${sz * 2});r.push(u.length);b.resize(0);r.push(u.length,u.byteOffset);return r})()`,
    `(function(){${pre}var u=new ${t}(b,${sz});var r=[u.length];b.resize(${sz * 6});r.push(u.length,u.byteOffset);b.resize(${sz});r.push(u.length,u.byteOffset);b.resize(${sz - 1});r.push(u.length,u.byteOffset);return r})()`,
    `(function(){${pre}var u=new ${t}(b,${sz},2);var r=[u.length];b.resize(${sz * 3});r.push(u.length);b.resize(${sz * 2});r.push(u.length,u.byteLength,u.byteOffset,u[0]);b.resize(${sz * 4});r.push(u.length,u.byteOffset);return r})()`,
    `(function(){${pre}var u=new ${t}(b);b.resize(${sz * 6});u[5]=${t.startsWith("Big") ? "7n" : "7"};return [u[5],u.at(-1),u.indexOf(${t.startsWith("Big") ? "7n" : "7"}),Object.keys(u).length,6 in u]})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});u[0]=${t.startsWith("Big") ? "1n" : "1"};return [u[0],Object.keys(u).length,0 in u,Object.getOwnPropertyNames(u).join()]})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});return Array.from(u)})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});return u.fill(${zero})})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});return u.subarray(0)})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});return u.slice()})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});return [u.length,u.byteLength,u.byteOffset,u.buffer===b]})()`,
    `(function(){${pre}var u=new ${t}(b);var s=u.subarray(1);b.resize(${sz * 6});return [s.length,s.byteOffset,u.subarray(1,3).length]})()`,
    `(function(){${pre}var u=new ${t}(b);var s=u.slice(1);b.resize(${sz * 6});return [s.length,s.buffer.resizable]})()`,
    `(function(){${pre}var u=new ${t}(b);var it=u.values();it.next();b.resize(${sz * 6});var c=0;while(!it.next().done)c++;return c})()`,
    `(function(){${pre}var u=new ${t}(b);return [Array.from(u.keys()).length,u.map(function(x){return x}).buffer.resizable,Array.from(u.entries()).length]})()`,
    `(function(){${pre}var u=new ${t}(b);var o=[];u.forEach(function(x,i){if(i===0)b.resize(${sz * 2});o.push(i)});return o})()`,
    `(function(){${pre}var u=new ${t}(b);return u.copyWithin(0,1)===u})()`,
    `(function(){${pre}var u=new ${t}(b);return Object.getOwnPropertyDescriptor(u,"3")})()`,
    `(function(){${pre}var u=new ${t}(b,0,2);b.resize(${sz});return Object.getOwnPropertyDescriptor(u,"0")})()`,
    `(function(){${pre}var u=new ${t}(b,${sz * 5});return u})()`,
    `(function(){${pre}b.resize(${sz * 4});var u=new ${t}(b,${sz * 4});return [u.length,u.byteOffset]})()`,
    `(function(){${pre}var u=new ${t}(b,0,5);return u})()`,
    `(function(){${pre}var u=new ${t}(b,0,8);return u})()`,
    `(function(){${pre}var u=new ${t}(b,0,4);return u.length})()`,
    `(function(){${pre}var u=new ${t}(b);b.transfer();return [u.length,u.byteLength,u.byteOffset]})()`,
    `(function(){${pre}var u=new ${t}(b);var c=new ${t}(u);b.resize(${sz * 6});return [c.length,c.buffer.resizable]})()`,
    `(function(){${pre}return ${t}.from(new ${t}(b)).buffer.resizable})()`,
    `(function(){${pre}var u=new ${t}(b);return new ${t}(u.buffer,0,u.length).length})()`);
}
// Atomics sobre buffer redimensionável e SAB growable.
for (const t of intTypes) {
  add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b);b.grow(16);return [u.length,Atomics.add(u,u.length-1,1),Atomics.load(u,u.length-1)]})()`,
    `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b,0,1);b.grow(16);return [u.length,Atomics.load(u,1)]})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b);b.resize(16);return [u.length,Atomics.store(u,u.length-1,3),Atomics.exchange(u,u.length-1,4),Atomics.compareExchange(u,u.length-1,4,5),Atomics.load(u,u.length-1)]})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b);b.resize(0);return Atomics.load(u,0)})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b,0,2);b.resize(1);return Atomics.add(u,0,1)})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b);return Atomics.add(u,{valueOf(){b.resize(0);return 0}},1)})()`,
    `(function(){var b=new ArrayBuffer(8,{maxByteLength:16});var u=new ${t}(b);return Atomics.add(u,0,{valueOf(){b.transfer();return 1}})})()`,
    `(function(){var b=new ArrayBuffer(8);var u=new ${t}(b);return Atomics.store(u,0,{valueOf(){b.transfer();return 1}})})()`);
}
add(`(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return [b.growable,b.byteLength,b.maxByteLength,b.grow(12),b.byteLength]})()`,
  `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(4)})()`, `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(17)})()`,
  `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});b.grow(8);return b.byteLength})()`, `(function(){var b=new SharedArrayBuffer(8);return b.grow(8)})()`,
  `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return [b.slice(2).growable,b.slice(2).byteLength]})()`, `SharedArrayBuffer.prototype.grow.call(new ArrayBuffer(8,{maxByteLength:16}),9)`,
  `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow(NaN)})()`, `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow(-1)})()`,
  `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow("12")&&b.byteLength})()`, `(function(){var b=new SharedArrayBuffer(8,{maxByteLength:16});return b.grow(Symbol())})()`);

// structuredClone é API de host, ausente no porte: os programas que a usam ficam fora.
const { knownPrograms, emitFactored } = require("./golden-prelude.js");
const known = new Set(knownPrograms("atomics_bun.tsv", (file) => file !== "atomics_bun.tsv"));
const jobs = [];
const seenSource = new Set();
let dup = 0;
for (const body of exprs) {
  if (body.includes("structuredClone")) continue;
  const source = PRE + "globalThis.R = F(function () { return " + body + " });";
  if (seenSource.has(source)) continue;
  seenSource.add(source);
  if (known.has(source)) { dup++; continue; }
  jobs.push({ body, source });
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
  const rows = [];
  let timeouts = 0, dropped = 0;
  for (let i = 0; i < jobs.length; i++) {
    const r = results[i];
    if (r.timedOut) { timeouts++; continue; }
    if (r.code !== 0 || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|\u2014|\u2013/.test(r.out)) {
      dropped++;
      process.stderr.write("descartado: " + jobs[i].body.slice(0, 160) + "\n");
      continue;
    }
    rows.push({ source: jobs[i].source, result: r.out });
  }
  process.stdout.write(emitFactored("atomics", rows));
  process.stderr.write(`programas ${rows.length}, descartados ${dropped}, timeouts ${timeouts}, repetidos dos goldens vizinhos ${dup}\n`);
}
main();
