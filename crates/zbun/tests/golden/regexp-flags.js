const a = /a/dgimsuy, b = /(?<year>\d{4})-(?<month>\d{2})/v, c = /[\p{L}--[a-z]]/v, d = /\u{1F600}/u, e = /a|b|c/, f = /(?:x)/, g = /\//g, h = /[\/]/, i = /\d+(?=x)(?<!y)/;
console.log(a, b, c, d, e, f, g, h, i, "x".replace(/x/g, "y"), /re/.test("re"), a / b / c);
