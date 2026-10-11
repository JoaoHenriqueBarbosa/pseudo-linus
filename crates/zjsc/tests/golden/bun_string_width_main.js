// `Bun.stringWidth(text, options)` contra o bun 1.4.2: a forma da função, grafemas (ZWJ, bandeiras, keycaps, tom de
// pele, seletores de variação, jamo, conjuntas índicas, Prepend), controles, ANSI com e sem countAnsiEscapeCodes,
// ambíguos, o modo perCodePoint, surrogates soltos, as opções (string vazia, null, protótipo, getter, Proxy) e a
// conversão do argumento; no fim, 400 textos pseudoaleatórios montados de pontos difíceis, medidos nos seis modos. Roda
// como `/app/main.js`. Texto fora do ASCII sai de `String.fromCodePoint`, para o arquivo ser ASCII.
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.message];
  }
};
const t = (...points) => String.fromCodePoint(...points);
const ESC = t(0x1b);
const modes = [
  undefined,
  { countAnsiEscapeCodes: true },
  { ambiguousIsNarrow: false },
  { ambiguousIsNarrow: false, countAnsiEscapeCodes: true },
  { perCodePoint: true },
  { perCodePoint: true, ambiguousIsNarrow: false, countAnsiEscapeCodes: true },
];
const all = (text) => modes.map((mode) => Bun.stringWidth(text, mode));

const descriptor = Object.getOwnPropertyDescriptor(Bun, "stringWidth");
out("descriptor", descriptor.writable, descriptor.enumerable, descriptor.configurable, descriptor.value.name, descriptor.value.length, String(descriptor.value));

const samples = [
  "", "hello", "a b\tc", t(0x4e2d, 0x6587), t(0xd55c, 0xae00), t(0x3042, 0x30a2), t(0xff21, 0xff22), t(0xe9, 0xe8, 0xa7, 0xb0),
  t(0x1f468, 0x200d, 0x1f469, 0x200d, 0x1f467), t(0x1f1e7, 0x1f1f7), t(0x1f1e7), t(0x1f1e7, 0x1f1f7, 0x1f1fa), t(0x31, 0xfe0f, 0x20e3), t(0x23, 0x20e3),
  t(0x1f44d, 0x1f3fd), t(0x1f3fd), t(0x2764, 0xfe0f), t(0x2764, 0xfe0e), t(0x2764), t(0xa9, 0xfe0f), t(0xae, 0xfe0f), t(0xa9), t(0x61, 0xfe0f),
  t(0x4e2d, 0xfe0e), t(0x65, 0x301), t(0x65, 0x301, 0x302), t(0x301), t(0x1112, 0x1161, 0x11ab), t(0x1161), t(0x915, 0x94d, 0x937), t(0x915, 0x94d, 0x200d, 0x937),
  t(0x600, 0x4e2d), t(0x600, 0x61), t(0x110bd, 0x1f600), t(0x200b), t(0x200d), t(0xad), t(0x202e, 0x61), t(0x1f600, 0x200d), t(0x200d, 0x1f600),
  "a" + t(1) + "b" + t(0x7f) + t(0x85), "\r\n", ESC + "[31mred" + ESC + "[0m", ESC + "]8;;http://x" + t(7) + "link" + ESC + "]8;;" + t(7),
  t(0x9b) + "31m" + t(0x4e2d), t(0x9c) + "x", ESC, ESC + "[1m" + t(0x301), "a" + ESC + "[1m" + t(0x301), t(0x1f600) + ESC + "[1m" + t(0x1f3fd),
  String.fromCharCode(0xd800), String.fromCharCode(0xdc00) + "a", String.fromCharCode(0xd83d) + "x", t(0x1f600).repeat(3), "x".repeat(40) + t(0x4e2d),
  t(0x391, 0x3b1, 0x410, 0x430), t(0x2026, 0x2500, 0x25cb), t(0xfe0f), t(0xe0061), t(0x1f9d1, 0x200d, 0x1f384), t(0x1fa89), t(0x4dc0),
];
for (const sample of samples) {
  out("width", JSON.stringify(sample), all(sample));
}

for (const value of [undefined, null, 5, true, 10n, { toString: () => t(0x4e2d) }, [t(0x4e2d), "a"]]) {
  out("coerce", typeof value, attempt(() => Bun.stringWidth(value)));
}
out("no argument", attempt(() => Bun.stringWidth()), attempt(() => Bun.stringWidth(Symbol("s"))));
const wideText = t(0xa7) + ESC + "[1m";
for (const options of [null, 5, "x", { ambiguousIsNarrow: "" }, { ambiguousIsNarrow: "no" }, { ambiguousIsNarrow: 0 }, { ambiguousIsNarrow: null }, { countAnsiEscapeCodes: 1 }, { perCodePoint: "yes", ambiguousIsNarrow: false }]) {
  out("options", JSON.stringify(options), Bun.stringWidth(wideText, options));
}
Object.prototype.countAnsiEscapeCodes = true;
out("polluted prototype", Bun.stringWidth(wideText, {}), Bun.stringWidth(wideText, Object.create({ countAnsiEscapeCodes: true })));
delete Object.prototype.countAnsiEscapeCodes;
const reads = [];
const getterOptions = { get ambiguousIsNarrow() { reads.push("ambiguous"); return false; }, get countAnsiEscapeCodes() { reads.push("count"); return true; }, get perCodePoint() { reads.push("per"); return false; } };
out("getters", Bun.stringWidth(wideText, getterOptions), reads.join(","));
const trapped = [];
const proxy = new Proxy({ ambiguousIsNarrow: false }, { getOwnPropertyDescriptor(target, key) { trapped.push(String(key)); return Reflect.getOwnPropertyDescriptor(target, key); }, get(target, key) { trapped.push("get " + String(key)); return Reflect.get(target, key); } });
out("proxy", Bun.stringWidth(wideText, proxy), trapped.join(","));
out("throwing getter", attempt(() => Bun.stringWidth("x", { get countAnsiEscapeCodes() { throw new Error("boom"); } })));

// Textos pseudoaleatórios (gerador congruente fixo) de pontos que mexem com grafemas, larguras e ANSI.
const pool = [
  0x61, 0x20, 0x1b, 0x5b, 0x6d, 0x31, 0x3b, 0x9b, 0x9c, 0x9d, 0x7, 0x5c, 0x4e2d, 0xac00, 0x1112, 0x1161, 0x11ab, 0x301, 0x200d, 0x200c,
  0xfe0f, 0xfe0e, 0x20e3, 0x1f600, 0x1f3fd, 0x1f1e7, 0x1f1f7, 0x2764, 0xa9, 0xa7, 0x391, 0x915, 0x94d, 0x937, 0x600, 0x110bd, 0xd4e, 0xad,
  0x200b, 0xe9, 0x3042, 0xff21, 0x85, 0x0, 0xa, 0xd, 0x1f468, 0x1f469, 0x2026, 0xd800, 0xdc00,
];
let seed = 12345;
const next = () => {
  seed = (seed * 1103515245 + 12345) % 2147483648;
  return seed;
};
for (let index = 0; index < 400; index++) {
  const length = 1 + (next() % 12);
  let text = "";
  for (let k = 0; k < length; k++) {
    const point = pool[next() % pool.length];
    text += point >= 0xd800 && point <= 0xdfff ? String.fromCharCode(point) : String.fromCodePoint(point);
  }
  out("random", index, all(text).join(","));
}
