// Gera tests/golden/syntax-errors.tsv rodando no bun: para cada trecho de programa, "ok" se
// `new vm.Script` aceita, ou a mensagem do SyntaxError. O trecho sai com JSON.stringify.
// Uso: bun scripts/gen-syntax-errors-golden.js > tests/golden/syntax-errors.tsv
import vm from "node:vm";

const snippets = [
    "", ";", "var a = 1;", "let a = 1; let a = 2;", "const a;", "const a = 1; a = 2;", "var let = 1;",
    "let let = 1;", "function f() { return 1 }", "return 1;", "break;", "continue;", "a: a: 1;",
    "while (1) { break b; }", "if (1) function f() {}", "'use strict'; with (a) {}", "'use strict'; var eval;",
    "'use strict'; 010", "'use strict'; delete x;", "'use strict'; function f(a, a) {}", "function f(a, a) {}",
    "function f(a = 1, a) {}", "(a, a) => 1", "(a = 1) => { 'use strict' }", "x = {", "x = [", "x = (", "x = `",
    "x = `${", "x = 'abc", "x = \"abc\n\"", "/abc", "/(/", "/a/gg", "1 +", "1 + + 1", "+ +", "a ? b", "a ? b :",
    "a ?? b || c", "a || b ?? c", "a ** b", "-a ** b", "(-a) ** b", "a?.b = 1", "a?.b++", "new a?.b()", "a?.`x`",
    "async function f() { await }", "function f() { await 1 }", "async () => await", "function* g() { yield }",
    "function* g() { var yield; }", "yield = 1", "'use strict'; yield = 1", "class A { constructor() {} constructor() {} }",
    "class A extends B { constructor() { super(); } foo() { super(); } }", "class A { #a; #a; }", "class A { foo() { this.#b } }",
    "class A { static prototype() {} }", "class A { get constructor() {} }", "class { }", "class A { x = arguments }",
    "for (let x of y, z) {}", "for (var i = 0 of []) {}", "for (let of x) {}", "for await (x of y) {}",
    "for (const x in y) { x = 1 }", "for (;;", "for (a b c)", "switch (a) { default: default: }", "switch (a) { case }",
    "try {}", "try {} catch {} finally", "try {} catch (a, b) {}", "throw\n1", "do x; while (0) y", "if (a) else b",
    "var {a, a} = b", "let {a, a} = b", "var [a, ...b,] = c", "({a: 1} = 1)", "({a}) = 1", "[a + 1] = b", "({...{a}} = b)",
    "({get a() {}, get a() {}})", "({__proto__: 1, __proto__: 2})", "({a = 1})", "({a = 1} = {})", "(a, b) => {", "(a, b) =>",
    "() => {}()", "async (a, b) => {}", "async a => {}", "async\na => {}", "a => {} + 1", "x = a => {}\n(1)", "import a from 'b'",
    "export var a;", "import.meta", "import('a')", "import()", "new.target", "function f() { new.target }", "super.x",
    "({ foo() { super.x } })", "label: function* g() {}", "if (1) class A {}", "while (1) let\nx", "let\nlet = 1",
    "0b2", "0o8", "0xg", "1_", "1__0", "1e", "08.5", "09n", "1n.5", "1.n", "'\\u{110000}'", "'\\x4'", "'\\u12'", "`\\u{`",
    "\"\\08\"", "/[/", "/(?<a>x)(?<a>y)/", "/\\u{1}/u", "a\u2028b", "var \u2028", "var a\\u0020b", "var \\u0061 = 1", "v\\u0061r a",
    "/* unterminated", "// ok", "<!-- html comment\n1", "-->", "#!shebang\n1", "a\n#!x", "var a = #", "@", "a b", "a(", "a)", "a[", "a]",
    "{", "}", "a.", "a..b", "a.1", "...a", "a = ...b", "var 1", "var a b", "var a,", "let [", "let {", "const {a}", "var a = ;",
    "function", "function (", "function f(", "function f()", "function f() {", "function f(...a, b) {}", "function f(...a = []) {}",
    "(...a, b) => 1", "(a, ...b,) => 1", "async function* f() { yield await }", "x = async function await() {}",
    "function f() { 'use strict'; with(a); }", "function f(a = 1) { 'use strict'; }", "'use strict'; arguments = 1",
    "'use strict'; ({eval} = 1)", "'use strict'; implements = 1", "'use strict'; let\nlet", "a = {if: 1, class: 2}", "a.if.class",
    "var if", "var enum", "enum = 1", "await = 1", "async = 1", "of = 1", "let = 1", "static = 1", "get = 1", "set = 1",
    "({async *[a]() {}})", "({async get a() {}})", "({get a(x) {}})", "({set a() {}})", "({set a(x, y) {}})", "({set a(...x) {}})",
    "1 = 2", "a++ = 1", "++a++", "a + b = c", "(a, b) = 1", "[(a)] = 1", "[(a = 1)] = 1", "({a: (b)} = 1)", "({a: (b = 1)} = 1)", "for ((a) of b);",
    "for (let [a] = 1 of b);", "for (async of x);", "for (let in x);", "using x = 1", "await using x = 1", "{ using x = 1 }",
];

const out = [];
for (const source of snippets) {
    let result = "ok";
    try {
        new vm.Script(source);
    } catch (e) {
        result = e instanceof SyntaxError ? e.message : "!" + e.name + ": " + e.message;
    }
    out.push([JSON.stringify(source), result].join("\t"));
}
console.log(out.join("\n"));
