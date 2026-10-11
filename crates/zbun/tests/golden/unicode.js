const a = "héllo", b = "日本語", c = "  ", d = "😀", e = 'a\0b', f = "\x01\x7f", g = "\ud800", h = '\u{10FFFF}';
const t = `héllo ${a} 日本語`, r = /日本語/u;
const o = { "é": 1, é2: 2, 日本: 3 };
console.log(a, b, c, d, e, f, g, h, t, r, o);
