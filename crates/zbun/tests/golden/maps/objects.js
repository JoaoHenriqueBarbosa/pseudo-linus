const o = { a: 1, "b-c": 2, [d]: 3, e() { return 1; }, get g() { return 2; }, ...s };
const [p, , q = 2, ...r] = arr;
const { u, v: { w = 3 }, ...rest } = o;
var d, s, arr;
console.log(o, p, q, r, u, w, rest);
