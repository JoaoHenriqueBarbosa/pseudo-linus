// `Bun.hash` contra o bun 1.4.2: a forma (a função `hash` é o wyhash e carrega os algoritmos nomeados), o valor de
// cada algoritmo para textos, buffers, typed arrays de vários tipos, DataView, Blob, ArrayBuffer e tamanhos que cruzam
// os limites de bloco (0 a 300 bytes), as sementes (número, fração, negativo, enorme, BigInt, outros tipos) e a
// conversão do texto (UTF-8, surrogate solto). Roda como `/app/main.js`. Valores grandes saem com `String`, que
// distingue número de BigInt pelo `n` que o `typeof` mostra ao lado.
const show = (value) => (typeof value === "bigint" ? `${value}n` : typeof value === "number" ? String(value) : JSON.stringify(value));
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : show(value))).join(" "));
const attempt = (thunk) => {
  try {
    return show(thunk());
  } catch (error) {
    return `throw ${error && error.name}: ${error && error.message}`;
  }
};

const descriptor = Object.getOwnPropertyDescriptor(Bun, "hash");
out("descriptor", descriptor.writable, descriptor.enumerable, descriptor.configurable, typeof descriptor.value, descriptor.value.name, descriptor.value.length, String(descriptor.value));
out("own keys", Object.getOwnPropertyNames(Bun.hash).join(","));
const names = ["wyhash", "adler32", "crc32", "cityHash32", "cityHash64", "xxHash32", "xxHash64", "xxHash3", "murmur32v2", "murmur32v3", "murmur64v2", "rapidhash"];
for (const name of names) {
  const member = Object.getOwnPropertyDescriptor(Bun.hash, name);
  out("member", name, member.writable, member.enumerable, member.configurable, member.value.name, member.value.length, member.value === Bun.hash);
}

// Bytes 0, 1, 2, ... (mod 251) em prefixos de cada tamanho.
const bytes = new Uint8Array(300);
for (let index = 0; index < bytes.length; index++) bytes[index] = (index * 7 + 3) % 251;
const sizes = [0, 1, 2, 3, 4, 5, 7, 8, 9, 12, 13, 15, 16, 17, 20, 24, 25, 31, 32, 33, 40, 47, 48, 49, 56, 63, 64, 65, 80, 95, 96, 97, 128, 143, 144, 145, 200, 240, 255, 256, 300];
for (const name of ["hash", ...names]) {
  const fn = name === "hash" ? Bun.hash : Bun.hash[name];
  out("sizes", name, sizes.map((size) => show(fn(bytes.subarray(0, size)))).join(","));
  out("seeded", name, sizes.map((size) => show(fn(bytes.subarray(0, size), 0x9e3779b97f4a7c15n))).join(","));
}

const text = "hello world";
for (const name of ["hash", ...names]) {
  const fn = name === "hash" ? Bun.hash : Bun.hash[name];
  const results = [
    fn(text), fn(text, 1), fn(text, 1.9), fn(text, -1), fn(text, 2 ** 53), fn(text, 2 ** 64), fn(text, 2 ** 70), fn(text, NaN), fn(text, Infinity), fn(text, -Infinity),
    fn(text, 5n), fn(text, -5n), fn(text, 2n ** 64n + 7n), fn(text, 2n ** 80n), fn(text, "7"), fn(text, true), fn(text, null), fn(text, {}),
  ];
  out("seeds", name, results.map(show).join(","));
}
for (const name of ["hash", "crc32", "xxHash64", "cityHash64"]) {
  const fn = name === "hash" ? Bun.hash : Bun.hash[name];
  const inputs = [
    "", "a", "é", "中文", "😀", "\ud800", "x\udc00y", 5, true, null, undefined, { toString: () => "obj" }, [1, 2],
    new Uint16Array([1, 2, 3]), new Float64Array([1.5]), new DataView(new Uint8Array([9, 8, 7]).buffer, 1), new Uint8Array([1, 2, 3]).buffer,
    Buffer.from("hello world"), new Blob(["hello world"]), new Blob(["hello", " ", "world"]).slice(6), bytes.subarray(10, 20),
  ];
  out("inputs", name, inputs.map((input) => attempt(() => fn(input))).join(","));
}
out("no argument", attempt(() => Bun.hash()), attempt(() => Bun.hash.crc32()), attempt(() => Bun.hash(Symbol("s"))));
out("typeof", typeof Bun.hash("a"), typeof Bun.hash.crc32("a"), typeof Bun.hash.xxHash3("a"), typeof Bun.hash.cityHash32("a"), typeof Bun.hash.rapidhash("a"));
out("this", attempt(() => Bun.hash.call(5, "a")), attempt(() => new Bun.hash("a")));
const big = new Uint8Array(100000);
for (let index = 0; index < big.length; index++) big[index] = index & 0xff;
out("big", names.map((name) => show(Bun.hash[name](big))).join(","));
