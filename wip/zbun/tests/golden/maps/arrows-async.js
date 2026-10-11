const f = async (a, b) => { const c = await a; return c + b; };
const g = x => y => z => x + y + z;
async function* h() { yield 1; for await (const v of src()) yield v; }
function* gen() { yield* [1, 2]; }
const src = async function* () {};
(async () => { await f(1, 2); })();
console.log(g(1)(2)(3), gen, h);
