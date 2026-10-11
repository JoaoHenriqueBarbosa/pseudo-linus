const a = 'a"b', b = "a'b", c = 'a"b\'c', d = "a\"b'c", e = `a"b'c`, f = '\\', g = "\n\r\t\b\f\v\0", h = '\x00\x1f\x7f\x80\xff', i = "Ā￿", j = '\u{10000}', k = "😀", l = "\ud800x", m = 'x\udc00';
const n = "a\
b", o = 'line1 line2', p = " ", q = '\0';
const t = "😀", u = "\u{1F600}", v = "é", w = "é", x = "\u{e9}";
console.log(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, t, u, v, w, x);
