// Gera tests/golden/identifiers.txt no bun 1.4.2: para cada ponto de código, 1 se ele pode
// (testes sem ASI nem espaço possível: propriedade abreviada, chave de objeto e método de classe)
// começar um identificador e 1 se pode continuar um, como dois bitmaps em
// faixas "início fim início_ok continuação_ok".
const ok = (src) => { try { new Function(src); return 1; } catch { return 0; } };
let prev = null, start = 0; const out = [];
for (let c = 0; c <= 0x10ffff; c++) {
  if (c >= 0xd800 && c < 0xe000) continue;
  const ch = String.fromCodePoint(c);
  const v = `${ok("({" + ch + ":1," + ch + "})")}${(ok("({a" + ch + "b:1})") && ok("(class{a" + ch + "b(){}})"))}`;
  if (v !== prev) { if (prev !== null) out.push(`${start.toString(16)} ${prev}`); prev = v; start = c; }
}
out.push(`${start.toString(16)} ${prev}`);
console.log(out.join("\n"));
