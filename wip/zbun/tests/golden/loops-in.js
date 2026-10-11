const o = { a: 1 };
for (var i = ("a" in o); i;) { break; }
for (var j = ("a" in o) ? 1 : 2; j < 3; j++) {}
for (let k = (() => "a" in o)(); k;) { break; }
for (const x in o) {}
for (const x of [1]) {}
for (var y in o) {}
for (var z of [1]) {}
for (x.y in o) {}
for ([a, b] of [[1, 2]]) {}
for ({ a, b } of [{}]) {}
for (let [c, d] = [1, 2]; c < d; c++) {}
for (let i = 0, n = 3; i < n; ++i) {}
for (; ;) { break; }
for (async of => 1; ;) { break; }
for ((async) of []) {}
for (let in o) {}
var a, b, x = {}, async;
