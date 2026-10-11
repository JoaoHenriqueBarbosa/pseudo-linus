// `Bun.stripANSI(text)` contra o bun 1.4.2: a forma da função e o texto que sobra de cada tipo de sequência (CSI de
// 7 e de 8 bits, OSC terminado por BEL, ST de 8 bits e `ESC \`, DCS/SOS/PM/APC, escapes de um e de dois caracteres,
// CAN/SUB que abortam, sequência sem fim, ST solto), em Latin-1 e em UTF-16, e a conversão do argumento. Roda como
// `/app/main.js`. O texto sai em JSON para os controles ficarem visíveis.
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.message];
  }
};
const text = (...points) => String.fromCodePoint(...points);
const ESC = text(0x1b);

const descriptor = Object.getOwnPropertyDescriptor(Bun, "stripANSI");
out("descriptor", descriptor.writable, descriptor.enumerable, descriptor.configurable, descriptor.value.name, descriptor.value.length, String(descriptor.value));

const inputs = [
  "", "plain", ESC + "[31mred" + ESC + "[0m", ESC + "[1;32;40mx" + ESC + "[m", text(0x9b) + "31mc1" + text(0x9b) + "0m",
  ESC + "]8;;http://x" + text(7) + "link" + ESC + "]8;;" + text(7), ESC + "]0;title" + ESC + "\\after", ESC + "]0;t" + text(0x9c) + "after",
  text(0x9d) + "0;t" + text(7) + "z", ESC + "Pdcs" + ESC + "\\z", ESC + "Xsos" + text(0x9c) + "z", ESC + "^pm" + ESC + "\\z", ESC + "_apc" + ESC + "\\z",
  text(0x90) + "d" + text(0x9c) + "z", text(0x98) + "s" + text(0x9c) + "z", text(0x9e) + "p" + text(0x9c) + "z", text(0x9f) + "a" + text(0x9c) + "z",
  ESC + "7saved" + ESC + "8", ESC + "(Bx", ESC + "#8y", ESC + "c", ESC, ESC + ESC + "[1mz", ESC + "[31", ESC + "]unterminated",
  ESC + "[3" + text(0x18) + "after", ESC + "]x" + text(0x1a) + "after", ESC + text(0x18) + "q", "a" + text(0x9c) + "b", text(0x9c),
  ESC + text(0x9c) + "q", ESC + text(0x7f) + "q", ESC + text(0x1f) + "q", ESC + "[" + text(0x9c) + "q", "x" + ESC + "[2Jy" + ESC + "[Hz",
  text(0x4e2d) + ESC + "[1m" + text(0x6587) + ESC + "[0m", text(0x1f600) + ESC + "[33m!" + ESC + "[39m", ESC + "[38;2;255;0;0mrgb" + ESC + "[0m",
  ESC + "(" + text(0x4e2d) + "z", ESC + "[" + text(0x4e2d) + "m", text(0xe9) + ESC + "[1m" + text(0xe9),
];
for (const input of inputs) {
  out("strip", JSON.stringify(input), JSON.stringify(Bun.stripANSI(input)));
}
out("same string", Bun.stripANSI("abc") === "abc", typeof Bun.stripANSI(ESC + "[1m"));
for (const input of [undefined, null, 5, true, { toString: () => ESC + "[1mobj" }, [ESC + "[1m", "b"], 10n]) {
  out("coerce", String(typeof input), attempt(() => Bun.stripANSI(input)));
}
out("no argument", attempt(() => Bun.stripANSI()), attempt(() => Bun.stripANSI(Symbol("s"))));
