// Gera tests/golden/syntax-error-positions.tsv rodando no bun: para os mesmos trechos de
// tests/golden/syntax-errors.tsv, a posição que o bun expõe no SyntaxError de `new vm.Script`.
// Cada linha é `fonte (JSON) <TAB> linha <TAB> coluna`; "-" quando o trecho é aceito.
//
// O que o bun expõe: `ParserError::toErrorObject` chama `addErrorInfo(vm, error, line, source)`, que
// grava só `line` (setLine) e zera a coluna (`setColumn(0)`). A posição do token (JSTextPosition)
// não chega ao ErrorInstance, então a coluna do golden é sempre 0 e o teste Rust compara só a linha.
// Se `e.line` faltar, cai para o "t.js:LINHA" da primeira linha do `e.stack`.
// Uso: bun scripts/gen-syntax-error-positions-golden.js > tests/golden/syntax-error-positions.tsv
import fs from "node:fs";
import vm from "node:vm";

const golden = new URL("../tests/golden/syntax-errors.tsv", import.meta.url);
const sources = fs
    .readFileSync(golden, "utf8")
    .split("\n")
    .filter((line) => line.length > 0)
    .slice(0, 211)
    .map((line) => JSON.parse(line.slice(0, line.indexOf("\t"))));

const out = [];
for (const source of sources) {
    let line = "-";
    let column = "-";
    try {
        new vm.Script(source, { filename: "t.js" });
    } catch (e) {
        if (!(e instanceof SyntaxError)) {
            line = "!" + e.name;
        } else {
            let value = e.line;
            if (typeof value !== "number") {
                const match = /^t\.js:(\d+)/.exec(String(e.stack).split("\n")[0]);
                value = match ? Number(match[1]) : -1;
            }
            line = String(value);
            column = String(typeof e.column === "number" ? e.column : 0);
        }
    }
    out.push([JSON.stringify(source), line, column].join("\t"));
}
console.log(out.join("\n"));
