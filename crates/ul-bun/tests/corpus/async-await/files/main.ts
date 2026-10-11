const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
async function work(id: number, ms: number): Promise<string> { await sleep(ms); console.log('done', id); return `w${id}`; }
async function* gen(): AsyncGenerator<number> { for (let i = 0; i < 3; i++) { await sleep(1); yield i; } }
const results = await Promise.all([work(1, 20), work(2, 5), work(3, 10)]);
console.log(results);
for await (const v of gen()) console.log('gen', v);
try { await Promise.reject(new TypeError('nope')); } catch (e) { console.log((e as Error).name, (e as Error).message); }
