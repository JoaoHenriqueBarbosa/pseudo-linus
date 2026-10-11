const a = 1, b = 0.1, c = 1e21, d = -0, e = 0xff, f = 1_000, g = .5, h = 5., i = 1e-7, j = 123456789012345680000;
const big = 12345678901234567890n, neg = -5n;
const s1 = 'single', s2 = "double", s3 = `tpl`, s4 = 'it\'s', s5 = "say \"hi\"", s6 = 'a\nb\tc\\d', s7 = "é\u{1F600}x";
const mix = 'He said "it\'s"', back = `a${1}b${s1}c`, nested = `x${`y${1}`}z`;
const re1 = /ab+c/gi, re2 = /[/]\//u, re3 = /(?<name>x)\k<name>/;
const t = true, f2 = false, n = null, u = undefined, inf = Infinity, nan = NaN;
console.log(a, b, c, d, e, f, g, h, i, j, big, neg, s1, s2, s3, s4, s5, s6, s7, mix, back, nested, re1, re2, re3, t, f2, n, u, inf, nan);
