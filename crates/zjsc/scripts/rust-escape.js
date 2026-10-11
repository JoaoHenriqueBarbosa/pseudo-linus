// Escape comum dos geradores que escrevem .rs em src/: o travessão (U+2014) e o traço médio (U+2013) não podem
// aparecer literais em arquivo do repositório, então saem como `\u{2014}` / `\u{2013}`.
const fs = require("fs");

const DASHES = /[\u2013\u2014]/g;

/** Troca cada travessão e traço médio por `\u{...}` (vale em literal de string e em tsv). */
const escapeDashes = (text) => text.replace(DASHES, (c) => `\\u{${c.charCodeAt(0).toString(16)}}`);

/** Grava o fonte Rust gerado já sem travessão literal, mesmo em comentário que o gerador copiou do CLDR. */
function writeRustSource(file, text) {
  fs.writeFileSync(file, escapeDashes(text));
}

module.exports = { DASHES, escapeDashes, writeRustSource };
