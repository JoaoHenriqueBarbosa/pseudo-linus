// Gera tests/golden/regexp-syntax.tsv rodando no bun (o oráculo): para cada padrão e flags, "ok" se
// o `new RegExp` aceita, ou a mensagem do `SyntaxError` sem o prefixo
// "Invalid regular expression: " (flags inválidas saem com "!" e a mensagem inteira). O padrão sai com JSON.stringify, para caber numa linha.
// Uso: bun scripts/gen-regexp-syntax-golden.js > tests/golden/regexp-syntax.tsv

const patterns = [
    "", "a", "abc", "a|b|c", "(a)(b)", "(?:a)", "a*", "a+?", "a{2,3}", "a{3,2}", "a{2,}", "{", "a{", "}",
    "]", "[", "[a-z]", "[z-a]", "[\\d-z]", "[a-\\d]", "\\", "a\\", "(", ")", "(?", "(?<", "(?<a>x)",
    "(?<a>x)(?<a>y)", "(?<a>x)|(?<a>y)", "\\k<a>", "(?<a>x)\\k<a>", "\\k<b>(?<a>x)", "\\1", "(a)\\2",
    "(?=a)", "(?!a)", "(?<=a)", "(?<!a)", "(?=a)*", "(?<=a)+", "^*", "$+", "\\b*", "a**", "a?+", "+a",
    "*", "?", "x{99999999999}", "x{2147483648}", "\\u{110000}", "\\u{10ffff}", "\\p{L}", "\\p{Letter}",
    "\\p{Script=Greek}", "\\p{sc=Grek}", "\\p{Nope}", "\\P{Any}", "\\p{RGI_Emoji}", "[\\p{L}--\\p{N}]",
    "[[a-z]&&[aeiou]]", "[\\q{abc|d}]", "[a&&&b]", "[(]", "\\c", "\\cA", "\\c1", "[\\c1]", "\\x4", "\\xGG",
    "\\u12", "\\0", "\\00", "\\8", "[\\b]", "\\B", "(?i:a)", "(?-i:a)", "(?i-i:a)", "(?ii:a)", "(?m:^a$)",
    "(?s:.)", "(?x:a)", "a(?<=b(?<=c))", "((((((((((a))))))))))\\10", "[^]", "[]", ".", "\\/", "/",
    "\\-", "[\\-]", "\\_", "\\a", "\\$", "(?<\\u0041>.)", "(?<a\\u{10000}>.)", "(?<𝒜>.)", "(?<1a>.)",
];
const flagsList = ["", "u", "v", "i", "iu", "y", "gimsuy", "uv", "x", "dg"];

const out = [];
for (const pattern of patterns) {
    for (const flags of flagsList) {
        let result;
        try {
            new RegExp(pattern, flags);
            result = "ok";
        } catch (e) {
            result = e.message;
            const prefix = "Invalid regular expression: ";
            if (result.startsWith(prefix))
                result = result.slice(prefix.length);
            else
                result = "!" + result;
        }
        out.push([JSON.stringify(pattern), flags, result].join("\t"));
    }
}
console.log(out.join("\n"));
