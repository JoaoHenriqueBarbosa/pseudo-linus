// Gera `tests/golden/url_pattern_main.js`: o modelo `scripts/url_pattern_main.template.js` com os casos do
// `urlpatterntestdata.json` do WPT (só `pattern` e `inputs`; o esperado é a saída do bun).
//
// Uso: `bun scripts/gen-url-pattern-golden.js <urlpatterntestdata.json> > tests/golden/url_pattern_main.js`; o
// `.expected.txt` sai de `bun tests/golden/url_pattern_main.js` na imagem do oráculo.
const fs = require("node:fs");
const path = require("node:path");

const [dataPath] = process.argv.slice(2);
if (!dataPath) {
  console.error("uso: bun gen-url-pattern-golden.js <urlpatterntestdata.json>");
  process.exit(2);
}
const cases = JSON.parse(fs.readFileSync(dataPath, "utf8")).map(entry => {
  const out = { pattern: entry.pattern };
  if ("inputs" in entry) out.inputs = entry.inputs;
  return out;
});
const ascii = JSON.stringify(cases).replace(/[\u007f-￿]/g, c => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
const template = fs.readFileSync(path.join(__dirname, "url_pattern_main.template.js"), "utf8");
process.stdout.write(template.replace("__CASES__", () => ascii));
