// Gera tests/golden/number_unit_scandinavian_bun.tsv a partir do bun (o oráculo): a unidade `mile-scandinavian`
// (a 45ª de Intl.supportedValuesOf("unit")) em `en`, nos três unitDisplay, e o composto `mile-scandinavian-per-hour`.
// Uso (da raiz do crate): bun scripts/gen-number-unit-scandinavian-golden.js
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");

const programs = [];
for (const display of ["short", "narrow", "long"]) {
  for (const value of [1, 2.5, 0]) {
    programs.push(`new Intl.NumberFormat("en", ${JSON.stringify({ style: "unit", unit: "mile-scandinavian", unitDisplay: display })}).format(${value})`);
  }
}
programs.push(`new Intl.NumberFormat("en", ${JSON.stringify({ style: "unit", unit: "mile-scandinavian-per-hour" })}).format(3)`);
programs.push(`Intl.supportedValuesOf("unit").filter((u) => ["mile-scandinavian","mile"].includes(u)).join(",")`);
programs.push(`Intl.supportedValuesOf("unit").length + ""`);

const lines = programs.map((src) => {
  if (/[^\x20-\x7e]/.test(src)) throw new Error(`${src}: fonte precisa ser ASCII de uma linha, sem tab`);
  // O teste (tests/number_unit_scandinavian_bun_golden.rs) compara a string que o programa devolve, sem harness.
  const result = (0, eval)(src);
  if (typeof result !== "string") throw new Error(`${src}: o programa não devolveu string`);
  return `${src}\t${result}`;
});
fs.writeFileSync(path.join(root, "tests/golden/number_unit_scandinavian_bun.tsv"), require("./golden-prelude.js").assertPublicResult(lines.join("\n") + "\n"));
console.error(`${lines.length} casos`);
