// Gera tests/golden/regexp-exec.tsv no bun: para cada (padrão, flags, entrada), o resultado de
// `exec` em lastIndex 0 como "índice:[início,fim]..." (grupos não participantes como "-"), ou
// "null". Padrão e entrada saem com JSON.stringify. Só flags que o interpretador trata sozinho.
// Uso: bun scripts/gen-regexp-exec-golden.js > tests/golden/regexp-exec.tsv

const cases = [
    ["abc", "", ["abc", "xabcx", "ab", ""]],
    ["a|b|c", "", ["zzc", "a", "d"]],
    ["(a)(b)?", "", ["ab", "a", "b"]],
    ["(a*)*", "", ["aaa", "b", ""]],
    ["(a*)+", "", ["aaa", "b"]],
    ["(z)((a+)?(b+)?(c))*", "", ["zaacbbbcac"]],
    ["a{2,3}", "", ["a", "aa", "aaaa"]],
    ["a{2,3}?", "", ["aaaa"]],
    ["a+?b", "", ["aaab"]],
    ["^abc$", "", ["abc", "abc\n", "xabc"]],
    ["^abc$", "m", ["x\nabc\ny", "abc"]],
    [".", "", ["\n", " ", "a", "😀"]],
    [".", "s", ["\n"]],
    [".", "u", ["😀"]],
    ["\\d+", "", ["abc123def", "x"]],
    ["\\w+\\s\\w+", "", ["hello world"]],
    ["\\bfoo\\b", "", ["a foo b", "afoob"]],
    ["\\Bfoo", "", ["afoo", " foo"]],
    ["[a-c]+", "", ["xxabcabcxx"]],
    ["[^a-c]+", "", ["abcxyzabc"]],
    ["abc", "i", ["ABC", "aBc"]],
    ["[a-z]+", "i", ["HeLLo"]],
    ["\\u212a", "i", ["k", "K"]],
    ["\\u212a", "iu", ["k", "K"]],
    ["(?=a)a", "", ["a", "b"]],
    ["(?!a)b", "", ["b", "a"]],
    ["(?<=a)b", "", ["ab", "b", "cb"]],
    ["(?<!a)b", "", ["ab", "cb"]],
    ["(?<=(\\d+)(\\d+))$", "", ["1053"]],
    ["(a)\\1", "", ["aa", "ab"]],
    ["\\1(a)", "", ["a", "aa"]],
    ["(?<x>a)\\k<x>", "", ["aa"]],
    ["(?:a|ab)(?:c|bcd)(d*)", "", ["abcd"]],
    ["(a|ab)(c|bcd)(d*)", "", ["abcd"]],
    ["a*?", "", ["aaa"]],
    ["(?:)", "", ["abc"]],
    ["\\u{1f600}", "u", ["😀"]],
    ["[\\u{1f600}-\\u{1f64f}]", "u", ["😁"]],
    ["\\p{L}+", "u", ["abc é 123", "日本語"]],
    ["\\p{Lu}", "u", ["abC"]],
    ["[\\p{L}--[a-z]]", "v", ["abcD"]],
    ["(a+)+b", "", ["aaaaaaaaaaaaaaaaaaaaaac", "aaab"]],
    ["(x+x+)+y", "", ["xxxxxxxxxxxxxxxxxxxxxxy"]],
    ["(?:(a)|b)*", "", ["ab"]],
    ["(?:(a)|(b))+", "", ["ab", "ba"]],
    ["((a)|(b))*", "", ["abab"]],
    ["\\s+", "", [" \t\n ﻿x"]],
    ["\\S+", "", ["  ab  "]],
    ["[\\s\\S]", "", ["\n"]],
    ["a.c", "", ["a\nc", "abc"]],
    ["\\x41\\u0042\\103", "", ["ABC"]],
    ["[\\b]", "", ["\b"]],
    ["(?i:a)b", "", ["Ab", "AB"]],
    ["(?:a|b)+?c", "", ["ababc"]],
    ["^(?:a|ab|abc)$", "", ["abc", "ab"]],
    ["(a?)*?b", "", ["aab"]],
];

const out = [];
for (const [pattern, flags, inputs] of cases) {
    for (const input of inputs) {
        const m = new RegExp(pattern, flags).exec(input);
        let result = "null";
        if (m) {
            const parts = [String(m.index)];
            const withIndices = new RegExp(pattern, flags + "d").exec(input);
            for (const pair of withIndices.indices)
                parts.push(pair ? "[" + pair[0] + "," + pair[1] + "]" : "-");
            result = parts.join(" ");
        }
        out.push([JSON.stringify(pattern), flags, JSON.stringify(input), result].join("\t"));
    }
}
console.log(out.join("\n"));
