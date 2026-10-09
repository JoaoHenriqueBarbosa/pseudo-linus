const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/parse_double.tsv no bun 1.4.2: literais decimais e o double que Number()
// devolve (bits em hexadecimal). Number() de um literal decimal completo usa o parseDouble da WTF.
let seed = 0x9e3779b9;
function rnd() { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return seed >>> 0; }
const digits = (n) => Array.from({ length: n }, () => rnd() % 10).join("");
const cases = ["0", "1", "0.1", "1e400", "1e-400", "2.2250738585072011e-308", "4.9e-324", "2.4703282292062327e-324",
  "2.4703282292062328e-324", "9007199254740993", "179769313486231580793728971405303415079934132710037826936173778980444968292764750946649017977587207096330286416692887910946555547851940402630657488671505820681908902000708383676273854845817711531764475730270069855571366959622842914819860834936475292719074168444365510704342711559699508093042880177904174497791",
  "123456789012345678901234567890e-20", "0.30000000000000004", "1.7976931348623157e308", "1.7976931348623159e308"];
for (let i = 0; i < 3000; i++) {
  const int = digits(1 + rnd() % 25), frac = rnd() % 2 ? "." + digits(rnd() % 25) : "";
  const exp = rnd() % 2 ? "e" + (rnd() % 2 ? "-" : "") + (rnd() % 340) : "";
  cases.push(int + frac + exp);
}
const dv = new DataView(new ArrayBuffer(8));
emitRow(cases.map((s) => { dv.setFloat64(0, Number(s)); return s + "\t" + dv.getBigUint64(0).toString(16).padStart(16, "0"); }).join("\n"));
