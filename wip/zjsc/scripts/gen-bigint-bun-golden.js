// Gera tests/golden/bigint_bun.tsv rodando no bun (o oráculo). Cada linha é um programa (JSON) e o texto de
// `R` que ele grava: o resultado como string, ou `Nome: mensagem` quando lança. Os programas são pequenos
// (uma expressão por linha) e rodam por eval indireto; os pesados (1n << 1000000n, 10n ** 100000n) só
// guardam comprimento ou trechos. Nenhum resultado carrega caminho da máquina.
// Uso: timeout 120 bun scripts/gen-bigint-bun-golden.js > tests/golden/bigint_bun.tsv

const exprs = [];
const add = e => exprs.push(e);

const BITS = [1, 2, 7, 8, 31, 32, 33, 52, 53, 54, 63, 64, 65, 96, 127, 128, 129, 191, 192, 193, 255, 256, 257, 1000];
const big = bits => (1n << BigInt(bits)) - 1n; // 2^bits - 1
const lit = v => (v < 0n ? "(" + v + "n)" : v + "n");

// Literais.
for (const s of ["0x0n", "0xffn", "0XFFn", "0o17n", "0O17n", "0b1010n", "0B1010n", "1_000n", "0xFF_FFn", "0b1_0n", "0o7_7n",
    "0n", "-0n", "00n", "01n", "1__0n", "1_n", "0x_1n", "0_1n", "1.5n", "1e3n", "0xn", "0b2n", "0o8n", "9007199254740993n",
    "123456789012345678901234567890n", "-123456789012345678901234567890n", "0b" + "1".repeat(200) + "n", "0x" + "f".repeat(100) + "n"])
    add("@eval:" + s);

// Aritmética nos tamanhos de borda.
const values = [0n, 1n, -1n, 2n, -2n, 10n, -10n];
for (const b of BITS) values.push(big(b), -big(b), 1n << BigInt(b), -(1n << BigInt(b)));
const pairs = [];
for (let i = 0; i < values.length; i += 3) for (let j = 0; j < values.length; j += 4) pairs.push([values[i], values[j]]);
for (const [a, b] of pairs) {
    for (const op of ["+", "-", "*", "/", "%"]) add(`${lit(a)} ${op} ${lit(b)}`);
}
for (const [a, b] of [[2n, 0n], [2n, 10n], [-2n, 3n], [-2n, 64n], [3n, 100n], [-3n, 101n], [0n, 0n], [0n, 5n], [1n, 1000n], [-1n, 1001n],
    [7n, 1n], [2n, 200n], [10n, 30n], [-10n, 31n], [2n, -1n], [-1n, -1n], [1n, -5n], [0n, -1n], [5n, -100n], [2n, 1000n]])
    add(`${lit(a)} ** ${lit(b)}`);
for (const a of ["1n", "0n", "-5n", "5n"]) { add(`${a} / 0n`); add(`${a} % 0n`); add(`${a} / -0n`); }
add("(2n ** 64n) / 0n"); add("(-(2n ** 64n)) % 0n");
for (const [a, b] of [[7n, 2n], [-7n, 2n], [7n, -2n], [-7n, -2n], [6n, 3n], [-6n, 3n], [1n, 7n], [-1n, 7n], [0n, -7n]]) {
    add(`${lit(a)} / ${lit(b)}`); add(`${lit(a)} % ${lit(b)}`);
}

// Shifts.
for (const v of [0n, 1n, -1n, 5n, -5n, big(63), -big(63), big(64), -big(64), big(65), -big(65), big(128), -big(128), big(200), -big(200)]) {
    for (const s of [0n, 1n, 2n, 31n, 32n, 33n, 63n, 64n, 65n, 127n, 128n, 129n, 200n, 1000n, -1n, -2n, -64n, -65n, -1000n]) {
        add(`${lit(v)} << ${lit(s)}`);
        add(`${lit(v)} >> ${lit(s)}`);
    }
    add(`${lit(v)} >>> 0n`); add(`${lit(v)} >>> 1n`); add(`${lit(v)} >>> -1n`);
}
add("1n << 1000000n");
add("(1n << 1000000n).toString(16).length");
add("(1n << 1000000n).toString(2).length");
add("1n << 1073741824n"); add("1n << 4294967296n"); add("1n << 100000000000000000000n");
add("1n << (2n ** 64n)"); add("-1n << 100000000000000000000n"); add("1n >> 100000000000000000000n");
add("-1n >> 100000000000000000000n"); add("1n >> (2n ** 64n)"); add("-1n >> (2n ** 64n)"); add("0n << 100000000000000000000n");
add("1n << -100000000000000000000n"); add("-1n << -100000000000000000000n");
add("(1n << 100n) << 1n << 200n"); add("1n << 0x10000000000000000n");
add("(10n ** 100000n).toString().length");
add("(10n ** 100000n).toString(16).length"); add("(10n ** 100000n - 1n).toString().length");
add("(10n ** 100000n).toString().slice(0, 20)"); add("(10n ** 100000n).toString().slice(-20)");
add("(7n ** 50000n).toString().length");
add("10n ** 100000000n"); add("2n ** 1000000n === (1n << 1000000n)"); add("(2n ** 1000000n).toString().length");
add("2n ** 10000000000n"); add("0n ** 100000000000000n"); add("1n ** 100000000000000n"); add("(-1n) ** 100000000000001n");
add("(-2n) ** 1000001n < 0n"); add("2n ** 9999999999999999999n");
add("BigInt('9'.repeat(100000)).toString().length"); add("BigInt('9'.repeat(100000)) + 1n === 10n ** 100000n");
add("BigInt('1' + '0'.repeat(99999)).toString() === '1' + '0'.repeat(99999)");
add("(BigInt('7'.repeat(50000)) * BigInt('3'.repeat(50000))).toString().length");
add("(10n ** 100000n / 3n).toString().length"); add("(10n ** 100000n % 997n).toString()");

// Bitwise.
for (const [a, b] of [[0n, 0n], [1n, -1n], [-1n, -1n], [5n, 3n], [-5n, 3n], [5n, -3n], [-5n, -3n], [big(64), -big(64)], [big(65), big(63)],
    [-big(65), big(63)], [-big(128), -big(129)], [big(128), -big(1)], [-(1n << 64n), (1n << 64n) - 1n], [-(1n << 63n), -(1n << 64n)],
    [big(200), -big(100)], [-big(200), big(100)], [-big(200), -big(100)], [1n << 100n, -(1n << 100n)], [0n, -1n], [-1n, 0n]]) {
    for (const op of ["&", "|", "^"]) add(`${lit(a)} ${op} ${lit(b)}`);
}
for (const v of [0n, 1n, -1n, 5n, -5n, big(63), big(64), big(65), -big(64), -big(65), big(128), -big(128), big(1000), -big(1000), 1n << 64n, -(1n << 64n)])
    add(`~${lit(v)}`);

// Comparação.
const cmpOps = ["<", "<=", ">", ">=", "==", "!=", "===", "!=="];
const rhs = ["0", "1", "-1", "1.5", "-1.5", "0.5", "NaN", "Infinity", "-Infinity", "9007199254740992", "9007199254740993", "2**64", "2**63", "-(2**63)",
    "1e21", "1e308", "'1'", "'0'", "''", "' 1 '", "'1n'", "'0x10'", "'abc'", "'1.5'", "'-1'", "'9007199254740993'", "'18446744073709551616'", "true", "false", "null",
    "undefined", "[]", "[1]", "({})", "1n", "2n", "-1n", "Object(1n)", "{valueOf(){return 1}}", "'  \\n12  '", "'-0'", "'+1'", "'1e3'", "'0b11'", "'0o7'", "'-0x10'"];
for (const l of ["0n", "1n", "-1n", "2n ** 64n", "2n ** 63n", "9007199254740993n", "-(2n ** 64n)", "1n << 1024n"])
    for (const r of rhs) for (const op of cmpOps) add(`${l} ${op} ${r}`);
for (const op of ["<", "==", "==="]) { add(`0n ${op} -0`); add(`0n ${op} 0`); add(`1n << 1100n ${op} Infinity`); add(`-(1n << 1100n) ${op} -Infinity`); }
add("[3n, 1n, 2n, -5n, 10n].sort().join()"); add("[3n, 1, 2n, -5, 10n].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)).join()");
add("[1n, 2n].includes(1n)"); add("[1n].indexOf(1)"); add("[1n].includes(1)");

// Mistura de tipos.
for (const r of ["1", "1.5", "'1'", "true", "null", "undefined", "Symbol()", "[]", "({})", "NaN", "Infinity", "1n"])
    for (const op of ["+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>", ">>>"]) add(`1n ${op} ${r}`);
for (const l of ["1", "'1'", "true", "null", "undefined", "1.5"]) for (const op of ["+", "-", "*", "/", "%", "**", "&", "<<", ">>>"]) add(`${l} ${op} 1n`);
add("1n + ''"); add("'' + 1n"); add("`${-5n}`"); add("'x' + 2n ** 70n"); add("1n + {valueOf(){return 1n}}"); add("1n + {valueOf(){return 1}}");
add("1n + {toString(){return '2'}}"); add("1n + [2]"); add("[1n] + ''");
add("-1n"); add("-(2n ** 64n)"); add("-0n"); add("+1n"); add("+(-1n)"); add("+Object(1n)"); add("-Object(1n)"); add("~Object(5n)");
add("Number(1n) + 1"); add("Math.max(1n, 2n)"); add("Math.min(1n)"); add("Math.abs(1n)"); add("Math.floor(1n)"); add("Math.sqrt(4n)");
add("Math.max()"); add("Math.round(-1n)"); add("isNaN(1n)"); add("isFinite(1n)"); add("[1n].map(Math.sign)");
add("let x = 1n; x++; x"); add("let x = 1n; ++x"); add("let x = 1n; x--; x"); add("let x = -1n; --x"); add("let x = 1n; x++"); add("let x = 1n; x += 1"); add("let x = 1n; x += 1n");
add("let x = 5n; x **= 2n"); add("let x = 5n; x <<= 70n"); add("let x = 1n << 70n; x >>= 69n"); add("let x = 5n; x >>>= 1n"); add("let x = 5n; x &= 3n; x |= 8n; x ^= 1n");
add("let x = Object(1n); x++; x"); add("let x = 2n**64n; x++; x"); add("let x = -(2n**64n); x--; x");
add("1n ? 'a' : 'b'"); add("0n ? 'a' : 'b'"); add("!0n"); add("!1n"); add("0n || 'z'"); add("0n ?? 'z'"); add("1n && 2n"); add("!!(-0n)");
add("typeof 1n"); add("typeof Object(1n)"); add("typeof BigInt"); add("typeof (1n + 1n)"); add("typeof BigInt.prototype"); add("typeof Object(1n).valueOf()");
add("Object.is(0n, -0n)"); add("Object.is(1n, 1n)"); add("Object.is(1n, 1)"); add("Object.is(2n ** 64n, 2n ** 64n)"); add("Object.is(Object(1n), Object(1n))");
add("new Map([[1n, 'a']]).get(1n)"); add("new Map([[1n, 'a']]).get(1)"); add("new Map([[0n, 'a']]).get(-0n)"); add("new Set([1n, 1n, 2n ** 64n, 2n ** 64n, 1]).size");
add("new Map([[2n**100n, 'a']]).has(2n**100n)"); add("({[1n]: 2})[1]"); add("({1n: 2})['1']"); add("Object.keys({[2n**64n]: 1})[0]"); add("[10,20,30][1n]");
add("'abc'[1n]"); add("[10,20,30][2n**64n]");
add("JSON.stringify(1n)"); add("JSON.stringify({a: 1n})"); add("JSON.stringify([1n])");
add("BigInt.prototype.toJSON = function(){return this.toString()}; JSON.stringify({a: 12n})");
add("JSON.stringify(Object(1n))"); add("JSON.parse('1n')"); add("JSON.stringify({a: 1n}, (k, v) => typeof v === 'bigint' ? String(v) : v)");
add("structuredClone(1n)"); add("structuredClone({a: 2n ** 70n}).a");
add("String(1n)"); add("`${2n ** 100n}`"); add("1n.toString()"); add("(1n).toString()"); add("Object(5n).toString()");
add("BigInt.prototype.toString.call(1)"); add("BigInt.prototype.valueOf.call(1)"); add("BigInt.prototype.valueOf.call(Object(3n))"); add("BigInt.prototype.toLocaleString.call(1)");

// BigInt(valor).
for (const v of ["0", "1", "-1", "1.5", "-1.5", "0.1", "1e21", "1e100", "2**53", "2**64", "-(2**63)", "2**1023", "Number.MAX_VALUE", "Number.MAX_SAFE_INTEGER",
    "Number.MIN_SAFE_INTEGER", "Number.EPSILON", "NaN", "Infinity", "-Infinity", "-0", "5e-324", "1e300", "9007199254740993", "123456789012345680000",
    "true", "false", "null", "undefined", "Symbol()", "Symbol.iterator", "''", "'  '", "' 12 '", "'\\n12\\t'", "'12n'", "'1_0'", "'0x'", "'0xff'", "'0XFF'",
    "'0o17'", "'0b101'", "'-0x10'", "'+0x10'", "'+5'", "'-5'", "'--5'", "'1.5'", "'1e3'", "'.5'", "'5.'", "'Infinity'", "'NaN'", "'abc'", "'0b2'", "'0o8'",
    "'0xg'", "'00012'", "'-0'", "'123456789012345678901234567890'", "'-123456789012345678901234567890'", "'0x' + 'f'.repeat(65)", "'9'.repeat(65)", "' \\u00a0 7'",
    "'\\ufeff7'", "'7\\u2028'", "'1 2'", "'٣'", "'１２'", "'0x1n'", "'  -  5'", "'-'", "'+'", "'0b'", "'0o'", "'0B11'", "'0O17'", "'1,000'",
    "1n", "Object(1n)", "Object(1)", "Object('12')", "[]", "[7]", "[1,2]", "({})", "{valueOf(){return 3}}", "{valueOf(){return '4'}}", "{toString(){return '5'}}",
    "{valueOf(){return 1.5}}", "{valueOf(){return {}}, toString(){return '9'}}", "new Date(5)", "()=>1", "BigInt", "new Number(2.5)", "new Boolean(true)", "[[]]", "['0x10']",
    "Number.MAX_SAFE_INTEGER + 2", "-(2**53)", "2**31", "-(2**31)", "2**32", "4294967296.5", "1e16", "1e15 + 0.5", "0.5", "-0.5", "1e-7"])
    add(`BigInt(${v})`);
add("new BigInt(1)"); add("BigInt()"); add("BigInt.length"); add("BigInt.name"); add("Object.getPrototypeOf(1n) === BigInt.prototype");
add("BigInt.prototype[Symbol.toStringTag]"); add("Object.prototype.toString.call(1n)"); add("1n instanceof BigInt"); add("Object(1n) instanceof BigInt");
add("Object.getOwnPropertyNames(BigInt).sort().join()"); add("Object.getOwnPropertyNames(BigInt.prototype).sort().join()");
add("BigInt.prototype.constructor === BigInt"); add("BigInt.asIntN.length"); add("BigInt.asUintN.length"); add("BigInt.prototype.toString.length"); add("BigInt.prototype.toLocaleString.length");

// asIntN / asUintN.
const asVals = [0n, 1n, -1n, 127n, 128n, 255n, 256n, -128n, -129n, big(63), 1n << 63n, big(64), 1n << 64n, -(1n << 63n), -(1n << 64n), -big(64), big(65), -big(65), 1n << 127n,
    big(128), -big(128), 1n << 200n, -(1n << 200n), big(200), -big(200), 12345678901234567890123456789n, -12345678901234567890123456789n];
for (const v of asVals) {
    for (const b of [0, 1, 2, 3, 7, 8, 9, 15, 16, 31, 32, 33, 53, 62, 63, 64, 65, 66, 100, 127, 128, 129, 150, 191, 192, 193, 199, 200]) {
        add(`BigInt.asIntN(${b}, ${lit(v)})`);
        add(`BigInt.asUintN(${b}, ${lit(v)})`);
    }
}
for (const a of ["-1", "1.5", "'3'", "undefined", "NaN", "2**53", "2**53 - 1", "Infinity", "-0", "true", "null", "1n", "Symbol()", "{valueOf(){return 8}}", "[]", "'abc'", "2**53 + 1"]) {
    add(`BigInt.asIntN(${a}, 255n)`); add(`BigInt.asUintN(${a}, 255n)`);
}
for (const a of ["1", "1.5", "'3'", "undefined", "null", "true", "Symbol()", "[]", "{}", "'12'", "NaN"]) { add(`BigInt.asIntN(8, ${a})`); add(`BigInt.asUintN(8, ${a})`); }
add("BigInt.asIntN(2**53 - 1, 5n)"); add("BigInt.asUintN(2**53 - 1, 5n)"); add("BigInt.asIntN(2**53 - 1, -5n)"); add("BigInt.asUintN(2**53 - 1, -5n)");
add("BigInt.asUintN(1000000, -1n).toString(16).length"); add("BigInt.asUintN(10000000, -1n)"); add("BigInt.asIntN(10000000, -1n)");
add("BigInt.asUintN(4294967296, -1n)");
add("BigInt.asIntN(64, 2n ** 63n)"); add("BigInt.asIntN(64, 2n ** 63n - 1n)"); add("BigInt.asUintN(64, -1n)"); add("BigInt.asUintN(64, 2n ** 64n)");

// toString(radix).
const bigs = [0n, 1n, -1n, 35n, 36n, 255n, big(32), big(53), big(63), big(64), -big(64), big(65), big(127), big(128), -big(129), 12345678901234567890123456789012345678901234567890n,
    -(10n ** 40n), 1n << 100n, big(300), -(1n << 1000n) - 1n];
for (const v of bigs) for (const r of [2, 3, 4, 5, 7, 8, 10, 11, 16, 17, 20, 31, 32, 33, 36]) add(`${lit(v)}.toString(${r})`);
for (const r of ["1", "37", "0", "-2", "2.5", "'16'", "undefined", "NaN", "null", "Infinity", "'x'", "2**32 + 2", "10.9", "{valueOf(){return 16}}", "36.99", "1n", "Symbol()", "true"])
    add(`255n.toString(${r})`);
add("(2n ** 1000n).toString(36)"); add("(2n ** 1000n - 1n).toString(2).length"); add("(-(3n ** 500n)).toString(7)"); add("(36n ** 100n).toString(36)");
add("(36n ** 100n - 1n).toString(36)"); add("(3n ** 999n).toString(3).length"); add("(5n ** 300n).toString(25).length"); add("(7n ** 400n).toString(32)");

// toLocaleString.
for (const v of ["0n", "1n", "-1n", "999n", "1000n", "-1000n", "1234567n", "-1234567890n", "123456789012345678901234567890n", "2n ** 100n", "-(2n ** 70n)", "10n ** 20n", "99999n", "100000n"])
    for (const loc of ["undefined", "'en'", "'en-US'", "'pt-BR'", "'de'", "'de-DE'", "'ar'", "'ar-EG'", "'hi'", "'hi-IN'", "'fr'", "'ja'", "'en-IN'", "'es'", "'ru'", "'sv'", "'fa'", "'bn'"])
        add(`${v}.toLocaleString(${loc})`);
add("1234567n.toLocaleString('en', {style: 'currency', currency: 'USD'})"); add("1234567n.toLocaleString('pt-BR', {style: 'currency', currency: 'BRL'})");
add("1234567n.toLocaleString('en', {useGrouping: false})"); add("12n.toLocaleString('en', {minimumFractionDigits: 2})"); add("1234567n.toLocaleString('en', {notation: 'compact'})");
add("1234567n.toLocaleString('de', {style: 'unit', unit: 'kilometer'})"); add("5n.toLocaleString('en', {style: 'percent'})"); add("1234567n.toLocaleString('en', {maximumSignificantDigits: 2})");
add("1234567n.toLocaleString('zzzz-invalid-locale-')"); add("1n.toLocaleString('en', {style: 'bad'})"); add("(2n**200n).toLocaleString('en', {notation: 'scientific'})");
add("new Intl.NumberFormat('en').format(2n ** 80n)"); add("new Intl.NumberFormat('pt-BR').format(-(2n ** 80n))"); add("new Intl.NumberFormat('en').format('123456789012345678901234567890')");
add("new Intl.NumberFormat('en').formatToParts(1234567n).map(p => p.type + ':' + p.value).join()"); add("new Intl.NumberFormat('de').format(12345678901234567890n)");

// Conversões Number / parseInt.
for (const v of ["0n", "1n", "-1n", "2n ** 53n", "2n ** 53n + 1n", "2n ** 64n", "-(2n ** 64n)", "2n ** 1023n", "2n ** 1024n", "-(2n ** 1024n)", "(2n ** 1024n) - 1n", "(2n ** 1024n) - 2n ** 970n",
    "(2n ** 1024n) - 2n ** 971n", "(2n ** 1024n) - 2n ** 970n - 1n", "2n ** 53n + 3n", "2n ** 54n + 2n", "2n ** 54n + 6n", "2n ** 54n + 1n", "2n ** 100n + 2n ** 47n", "2n ** 100n + 2n ** 47n + 1n",
    "2n ** 100n + 2n ** 48n", "9007199254740993n", "9007199254740995n", "123456789012345678901234567890n", "10n ** 308n", "10n ** 309n", "10n ** 400n"]) {
    add(`Number(${v})`); add(`parseInt(${v})`); add(`parseFloat(${v})`); add(`${v} == Number(${v})`);
    add(`Number.isInteger(${v})`); add(`Number.isSafeInteger(${v})`); add(`parseInt(${v}, 16)`); add(`+String(${v})`); add(`Number.parseFloat(String(${v}))`);
}
add("Number(Object(5n))"); add("Number('5n')"); add("parseInt('5n')"); add("parseInt('12345678901234567890')"); add("BigInt(parseInt('12345678901234567890'))"); add("BigInt(12345678901234567890)");
add("BigInt('12345678901234567890')"); add("BigInt(Number('12345678901234567890'))"); add("Number.MAX_SAFE_INTEGER + 2 === 2**53 + 1");
add("BigInt(Number.MAX_SAFE_INTEGER) + 2n"); add("BigInt(2**53) + 1n"); add("Number(2n**64n) === 2**64"); add("Number.isInteger(5n)"); add("Number.isNaN(5n)");
add("5n == 5"); add("5n === 5"); add("5n == '5'"); add("5n == Object(5n)"); add("Object(5n) == Object(5n)"); add("[5n] == 5"); add("[5n] == '5'");
add("Number.parseInt('0x' + 255n.toString(16))"); add("Number.prototype.toString.call(5n)"); add("(5).toFixed.call(5n)");
add("Math.pow(2n, 2n)"); add("2 ** 2n"); add("2n ** 2"); add("Number(2n) ** 2"); add("BigInt(2) ** 100n"); add("BigInt(Math.pow(2, 100))");
add("BigInt(Number.MAX_VALUE).toString().length"); add("BigInt(Number.MAX_VALUE) === (2n ** 1024n - 2n ** 971n)");

// TypedArrays.
for (const ta of ["BigInt64Array", "BigUint64Array"]) {
    add(`${ta}.BYTES_PER_ELEMENT`); add(`${ta}.name`); add(`new ${ta}(2).join()`); add(`new ${ta}([1n, 2n]).join()`); add(`new ${ta}([1, 2])`); add(`new ${ta}([1.5])`);
    add(`new ${ta}(['1', '0x10', true]).join()`); add(`new ${ta}(['abc'])`); add(`new ${ta}([undefined])`); add(`new ${ta}([null])`); add(`new ${ta}([Symbol()])`);
    add(`new ${ta}([-1n, 2n**63n, 2n**64n, 2n**64n + 5n, -(2n**63n), -(2n**63n) - 1n, 2n**100n + 7n]).join()`);
    add(`const a = new ${ta}(2); a[0] = 2n**64n + 3n; a[1] = -5n; a.join()`); add(`const a = new ${ta}(1); a[0] = 1`); add(`const a = new ${ta}(1); a[0] = '5'; a[0]`);
    add(`const a = new ${ta}(2); a.set([1n, 2n]); a.join()`); add(`const a = new ${ta}(2); a.set([1, 2])`); add(`const a = new ${ta}(2); a.set(new ${ta}([9n, -9n])); a.join()`);
    add(`new ${ta}(2).set(new Int8Array(1))`); add(`new Int8Array(1).set(new ${ta}(1))`); add(`new Float64Array(new ${ta}(1))`); add(`new ${ta}(new Int8Array(1))`);
    add(`new ${ta}([3n, 1n, 2n]).sort().join()`); add(`new ${ta}([3n, 1n, -2n]).sort().join()`); add(`new ${ta}([1n, 2n]).map(x => x * 2n).join()`); add(`new ${ta}([1n, 2n]).map(x => 1)`);
    add(`new ${ta}([1n, 2n, 3n]).fill(7n).join()`); add(`new ${ta}(3).fill(7)`); add(`new ${ta}([1n, 2n, 3n]).includes(2n)`); add(`new ${ta}([1n, 2n, 3n]).indexOf(2)`);
    add(`new ${ta}([1n, 2n, 3n]).reduce((a, b) => a + b)`); add(`Array.from(new ${ta}([1n, 2n])).join()`); add(`new ${ta}([1n,2n]).toString()`); add(`new ${ta}([5n, 6n]).at(-1)`);
    add(`new ${ta}(new ArrayBuffer(16)).length`); add(`new ${ta}(new ArrayBuffer(12))`); add(`new ${ta}(new ArrayBuffer(16), 8).length`); add(`new ${ta}(new ArrayBuffer(16), 4)`);
    add(`new ${ta}([1n]).slice().join()`); add(`new ${ta}([1n,2n,3n]).subarray(1).join()`); add(`Object.prototype.toString.call(new ${ta}(1))`);
    add(`new ${ta}([1n,2n]).reverse().join()`); add(`new ${ta}([1n,2n]).toSorted().join()`); add(`new ${ta}([1n,2n]).with(0, 9n).join()`); add(`new ${ta}([1n,2n]).with(0, 9)`);
    add(`[...new ${ta}([1n, 2n]).entries()].join('|')`); add(`new ${ta}([2n**63n]).join()`); add(`new ${ta}([2n**63n - 1n]).join()`);
    for (const a of ["Add", "Sub", "And", "Or", "Xor", "Exchange"])
        add(`const a = new ${ta}(2); a[0] = 10n; const r = [Atomics.${a.toLowerCase()}(a, 0, 6n), a[0]]; r.join()`);
    add(`const a = new ${ta}(1); Atomics.store(a, 0, -1n); a[0]`); add(`const a = new ${ta}(1); Atomics.store(a, 0, 2n**64n + 9n); a[0]`); add(`const a = new ${ta}(1); Atomics.store(a, 0, 5)`);
    add(`const a = new ${ta}(1); Atomics.add(a, 0, 1)`); add(`const a = new ${ta}(1); Atomics.load(a, 0)`); add(`const a = new ${ta}(1); a[0] = 7n; Atomics.compareExchange(a, 0, 7n, 8n); a[0]`);
    add(`const a = new ${ta}(1); a[0] = 7n; Atomics.compareExchange(a, 0, 6n, 8n); a[0]`); add(`const a = new ${ta}(1); Atomics.store(a, 0, 5n)`); add(`Atomics.isLockFree(8)`);
    add(`const a = new ${ta}(new SharedArrayBuffer(16)); Atomics.add(a, 1, 3n); Atomics.load(a, 1)`); add(`const a = new ${ta}(1); Atomics.notify(a, 0)`);
    add(`const a = new ${ta}(new SharedArrayBuffer(8)); Atomics.wait(a, 0, 1n, 0)`); add(`const a = new ${ta}(new SharedArrayBuffer(8)); Atomics.wait(a, 0, 0n, 1)`);
}
add("new DataView(new ArrayBuffer(16)).getBigInt64(0)"); add("const d = new DataView(new ArrayBuffer(16)); d.setBigInt64(0, -2n); [d.getBigInt64(0), d.getBigUint64(0), d.getBigUint64(0, true)].join()");
add("const d = new DataView(new ArrayBuffer(16)); d.setBigUint64(0, 2n**64n + 5n); d.getBigUint64(0)"); add("const d = new DataView(new ArrayBuffer(16)); d.setBigInt64(0, 5)");
add("const d = new DataView(new ArrayBuffer(16)); d.setBigInt64(0, 2n**63n); d.getBigInt64(0)"); add("const d = new DataView(new ArrayBuffer(4)); d.getBigInt64(0)");
add("const d = new DataView(new ArrayBuffer(16)); d.setBigInt64(8, 0x0102030405060708n, true); new Uint8Array(d.buffer).join()");
add("new BigInt64Array([1n]) instanceof BigUint64Array"); add("Object.getPrototypeOf(BigInt64Array) === Object.getPrototypeOf(Int8Array)");

// Extras.
add("BigInt(Number.MAX_SAFE_INTEGER) * BigInt(Number.MAX_SAFE_INTEGER)"); add("(2n ** 64n - 1n) * (2n ** 64n - 1n)"); add("(2n ** 128n - 1n) ** 2n");
add("((2n ** 64n) ** 3n - 1n) / (2n ** 64n - 1n)"); add("(1n << 128n) / (1n << 64n)"); add("((1n << 128n) + 5n) % (1n << 64n)"); add("(10n ** 50n) / (10n ** 25n + 1n)");
add("(10n ** 50n) % (10n ** 25n + 1n)"); add("(-(10n ** 50n)) / (10n ** 25n + 1n)"); add("(-(10n ** 50n)) % (10n ** 25n + 1n)"); add("(10n ** 50n) % -(10n ** 25n + 1n)");
add("(2n**192n - 1n) / (2n**64n + 1n)"); add("(2n**192n - 1n) % (2n**64n + 1n)"); add("(2n**128n) / (2n**64n - 1n)"); add("(2n**1000n) / 3n"); add("(2n**1000n) % 3n");
add("(2n**1000n) / (2n**500n - 1n)"); add("(2n**1000n) % (2n**500n - 1n)"); add("(3n**500n) / (3n**250n + 7n)"); add("(3n**500n) % (3n**250n + 7n)");
add("(2n**64n) / 1n"); add("(2n**64n) / -1n"); add("(-(2n**63n)) / -1n"); add("(-(2n**64n)) % (2n**64n)"); add("(2n**64n) % (2n**64n)"); add("(2n**64n - 1n) % (2n**64n)");
add("0n / 5n"); add("0n % 5n"); add("-0n / 5n"); add("(-5n) % 5n"); add("(-5n) % 5n === 0n"); add("Object.is(-5n % 5n, 0n)");
add("BigInt.asUintN(64, 0xffffffffffffffffn * 0xffffffffffffffffn)"); add("BigInt.asUintN(64, 6364136223846793005n * 1442695040888963407n + 1n)");
add("(1n << 64n) - 1n === 0xffffffffffffffffn"); add("0xffffffffffffffffn + 1n === 1n << 64n"); add("-(2n ** 63n) === -9223372036854775808n"); add("2n ** 63n - 1n");
add("5n > 3n && 3n < 5n"); add("2n**64n > 2n**63n"); add("-(2n**64n) < -(2n**63n)"); add("2n**64n == 18446744073709551616n"); add("2n**64n == 18446744073709551616");
add("2n**64n == 18446744073709551617"); add("2n**64n < 18446744073709551617"); add("2n**64n + 1n > 2**64"); add("2n**64n + 1n == 2**64"); add("2n**1024n > Number.MAX_VALUE");
add("2n**1024n == Infinity"); add("-(2n**1024n) < -Number.MAX_VALUE"); add("1n < '1.5'"); add("2n > '1.5'"); add("1n < 1.5"); add("2n < 1.5"); add("-1n > -1.5"); add("-2n < -1.5");
add("9007199254740993n > 9007199254740992"); add("9007199254740993n == 9007199254740992"); add("9007199254740993n < 9007199254740994");
add("let s = 0n; for (let i = 1n; i <= 100n; i++) s += i; s"); add("let f = 1n; for (let i = 2n; i <= 50n; i++) f *= i; f"); add("let a = 0n, b = 1n; for (let i = 0; i < 300; i++) [a, b] = [b, a + b]; a");
add("let f = 1n; for (let i = 2n; i <= 1000n; i++) f *= i; f.toString().length"); add("let f = 1n; for (let i = 2n; i <= 1000n; i++) f *= i; f.toString().slice(0, 30)");
add("function gcd(a, b) { while (b) [a, b] = [b, a % b]; return a } gcd(2n**100n - 1n, 2n**60n - 1n)");
add("function modpow(b, e, m) { let r = 1n; b %= m; while (e > 0n) { if (e & 1n) r = r * b % m; b = b * b % m; e >>= 1n } return r } modpow(3n, 10n**20n, 10n**9n + 7n)");
add("function isqrt(n) { let x = n, y = (x + 1n) >> 1n; while (y < x) { x = y; y = (x + n / x) >> 1n } return x } isqrt(10n**60n)");
add("BigInt('0b' + '1'.repeat(70))"); add("BigInt('0o' + '7'.repeat(30))"); add("BigInt('0x' + 'abc'.repeat(30))"); add("BigInt('-0x1')"); add("BigInt('1'.repeat(30))");
add("BigInt.prototype.hasOwnProperty('toLocaleString')"); add("Reflect.ownKeys(BigInt.prototype).map(String).join()"); add("Object.getOwnPropertyDescriptor(BigInt.prototype, Symbol.toStringTag).writable");
add("BigInt.prototype.toString.call(Object(2n**70n), 16)"); add("BigInt.prototype.valueOf.call({})"); add("BigInt.prototype.toString.call('1')");
add("Object(1n) + 1n"); add("Object(1n) * Object(2n)"); add("Object(1n) < Object(2n)"); add("Object(1n) == 1n"); add("Object(1n) === 1n"); add("Object(1n).valueOf() === 1n");
add("Object(2n**70n) + 1n"); add("[1n, 2n].toString()"); add("`${[1n, 2n**64n]}`"); add("String([[1n]])"); add("({}).toString.call(Object(1n))"); add("Object.keys(Object(1n)).length");
add("Symbol() + 1n"); add("1n + Symbol()"); add("BigInt(Symbol())"); add("BigInt(Symbol.iterator)"); add("Number(Symbol())");
add("new Intl.PluralRules('en').select(1n)"); add("new Intl.PluralRules('pt').select(2n ** 70n)"); add("new Intl.NumberFormat('en', {notation: 'compact'}).format(10n ** 12n)");
add("new Intl.RelativeTimeFormat('en').format(1n, 'day')"); add("new Date(1n)"); add("new Date(2n ** 64n)");
add("Array(3n)"); add("new Array(3n)"); add("'ab'.repeat(2n)"); add("'ab'.slice(1n)"); add("[1,2,3].slice(1n)"); add("(1.5).toFixed(1n)"); add("(255).toString(16n)");
add("parseInt('11', 2n)"); add("Number.prototype.toString.call(255, 2n)");

// BigInt gigantes: comprimento e hash FNV-1a (32 bits) do texto, calculados dentro do programa.
const fnv = "((s) => { let h = 0x811c9dc5; for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 0x01000193) >>> 0; } return h; })";
for (const [e, radix] of [["2n ** 100000n", 10], ["2n ** 100000n", 16], ["2n ** 100000n", 2], ["2n ** 100000n", 36], ["3n ** 50000n", 10],
    ["(2n ** 100000n) - 1n", 10], ["-(2n ** 100000n)", 10], ["10n ** 50000n + 7n", 10], ["7n ** 30000n", 7], ["7n ** 30000n", 31],
    ["(1n << 200000n) / 12345678901234567890n", 10], ["(5n ** 40000n) % (3n ** 20000n)", 10], ["(2n ** 100000n) * (3n ** 50000n)", 10],
    ["BigInt.asUintN(100000, -1n)", 16], ["BigInt.asIntN(100000, 2n ** 99999n)", 10], ["~(2n ** 100000n)", 10],
    ["(2n ** 100000n) ^ (3n ** 50000n)", 16], ["(2n ** 100000n) & -(3n ** 50000n)", 10], ["(2n ** 100000n) | -(3n ** 50000n)", 10]])
    add(`(() => { const t = (${e}).toString(${radix}); return t.length + ":" + ${fnv}(t); })()`);

console.error("expressões:", exprs.length);

const wrap = e => {
    // Literais: o erro de sintaxe sai pelo eval interno, onde o try do programa o pega.
    if (e.startsWith("@eval:")) return `try { R = String(eval(${JSON.stringify(e.slice(6))})); } catch (e) { R = e.name + ": " + e.message; }`;
    // Programas que começam com declaração viram função; a última instrução vira o valor devolvido.
    if (/^(let|const|function)\b/.test(e)) {
        const m = /^(.*[;}])\s*([^;{}]+)$/s.exec(e);
        return `try { R = String((() => { ${m[1]} return ${m[2]} })()); } catch (e) { R = e.name + ": " + e.message; }`;
    }
    if (/^BigInt\.prototype\.toJSON = /.test(e)) {
        const m = /^(.*?;)\s*(.*)$/s.exec(e);
        return `try { ${m[1]} R = String(${m[2]}); } catch (e) { R = e.name + ": " + e.message; }`;
    }
    return `try { R = String(${e}); } catch (e) { R = e.name + ": " + e.message; }`;
};

const out = [];
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
for (const e of exprs) {
    const src = wrap(e);
    if (seen.has(src)) continue;
    seen.add(src);
    let result;
    if (process.env.TRACE) console.error(e);
    try {
        globalThis.R = undefined;
        (0, eval)(src);
        result = typeof globalThis.R === "string" ? globalThis.R : "<undefined>";
    } catch (err) {
        continue; // o programa não chegou a gravar R (erro de sintaxe fora do try): fora do golden
    }
    delete BigInt.prototype.toJSON; // o programa do toJSON não pode vazar para os seguintes
    if (result.length > 4000) result = "#len" + result.length;
    if (/\/home\/|\/Users\//.test(result)) continue;
    out.push(JSON.stringify(src) + "\t" + JSON.stringify(result));
}
console.log(out.join("\n"));
console.error("programas:", out.length);
