const n = 1, s = "x";
const t1 = `a${n}b${s}c`, t2 = String.raw`\n${n}`, t3 = `multi
line ${n}
text`;
console.log(t1, t2, t3, tag`x${n}y`);
function tag(a, ...b) { return a.raw.join("") + b.join(""); }
