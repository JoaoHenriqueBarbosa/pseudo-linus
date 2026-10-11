// Gera tests/golden/bigint.tsv rodando no bun (o oráculo): para cada caso, as entradas em
// hexadecimal (sorteadas por um xorshift64* com semente fixa) e o resultado em várias bases.
// Resultados com mais de 600 caracteres saem como o FNV-1a de 32 bits do texto, para o arquivo não
// crescer: o teste calcula o mesmo hash sobre a saída do porte.
// Uso: bun scripts/gen-bigint-golden.js > tests/golden/bigint.tsv

let state = 0x9e3779b97f4a7c15n;
function nextWord() {
    state ^= state >> 12n;
    state ^= (state << 25n) & 0xffffffffffffffffn;
    state ^= state >> 27n;
    return (state * 0x2545f4914f6cdd1dn) & 0xffffffffffffffffn;
}

function randomBigInt(words) {
    let hex = "";
    for (let i = 0; i < words; i++)
        hex += nextWord().toString(16).padStart(16, "0");
    const negative = (nextWord() & 1n) === 1n;
    const value = BigInt("0x" + hex);
    return negative ? -value : value;
}

function fnv(text) {
    let h = 0x811c9dc5;
    for (let i = 0; i < text.length; i++) {
        h ^= text.charCodeAt(i);
        h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(16);
}

const hexOf = v => (v < 0n ? "-" : "") + (v < 0n ? -v : v).toString(16);
const RADIXES = [10, 16, 2, 7, 36];
const out = [];
const show = text => (text.length > 600 ? "#" + fnv(text) : text);
// Os tamanhos cruzam os limiares do porte: Comba fixo (2, 4, 8, 16 dígitos), Karatsuba, Toom-3 e a
// FFT (soma dos tamanhos a partir de 2300 dígitos de 64 bits, o menor com pelo menos 600).
const sizes = [1, 2, 3, 4, 5, 8, 13, 16, 20, 33, 40, 60, 100, 150, 250, 400, 700, 1200];
for (const size of sizes) {
    for (let rep = 0; rep < (size < 100 ? 6 : 2); rep++) {
        const a = randomBigInt(size);
        // Um caso em cada três é o quadrado (a mesma entrada dos dois lados).
        const b = rep % 3 === 2 ? a : randomBigInt(Math.max(1, size - (rep % 3)));
        out.push(["mul", hexOf(a), hexOf(b), ...RADIXES.map(r => show((a * b).toString(r)))].join("\t"));
        out.push(["str", hexOf(a), "", ...RADIXES.map(r => show(a.toString(r)))].join("\t"));
    }
}
for (const [base, exp] of [[3n, 1000n], [-7n, 333n], [2n, 4096n], [12345678901234567890n, 77n], [-1n, 99999n], [0n, 0n], [10n, 2000n]])
    out.push(["pow", hexOf(base), hexOf(exp), ...RADIXES.map(r => show((base ** exp).toString(r)))].join("\t"));
console.log(out.join("\n"));
