const a = "héllo wörld", b = "日本語のテキスト", c = "😀 emoji", d = `t${a}é${b}日本`;
const e = { "é": 1, 日本: 2 }; // comentário ç
console.log(a, b, c, d, e, "ünï", /é+/u);
function ç(é) { return é + "日本"; }
console.log(ç("x"));
