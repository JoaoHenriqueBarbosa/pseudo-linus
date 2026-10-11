const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, e.message); }
};
const drain = async (s) => { const out = []; for await (const c of s) out.push(c); return out; };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
(async () => {
  await show('shape', async () => [typeof ReadableStream.from, ReadableStream.from.length, ReadableStream.from.name]);
  await show('array', async () => await drain(ReadableStream.from([1, 2, 3])));
  await show('string', async () => await drain(ReadableStream.from('ab')));
  await show('set', async () => await drain(ReadableStream.from(new Set(['x', 'y']))));
  await show('generator', async () => await drain(ReadableStream.from((function* () { yield 'a'; yield 'b'; })())));
  await show('async generator', async () => await drain(ReadableStream.from((async function* () { yield 'a'; await sleep(1); yield 'b'; })())));
  await show('promises in sync iterable', async () => await drain(ReadableStream.from([Promise.resolve(1), 2])));
  await show('async iterable object', async () => await drain(ReadableStream.from({ [Symbol.asyncIterator]() { let i = 0; return { next: async () => (i < 2 ? { done: false, value: i++ } : { done: true }) }; } })));
  await show('instance', async () => { const s = ReadableStream.from([1]); return [s instanceof ReadableStream, s.locked, s.constructor.name]; });
  await show('not iterable', async () => ReadableStream.from(5));
  await show('object', async () => ReadableStream.from({}));
  await show('throwing iterator', async () => await drain(ReadableStream.from((function* () { yield 1; throw new Error('gen boom'); })())));
  await show('cancel calls return', async () => { let returned = null; const it = { [Symbol.iterator]() { return { next: () => ({ done: false, value: 1 }), return(v) { returned = v; return {}; } }; } }; const s = ReadableStream.from(it); const r = s.getReader(); await r.read(); await r.cancel('why'); return returned; });
  await show('lazy pull', async () => { let pulls = 0; const s = ReadableStream.from({ [Symbol.iterator]() { return { next: () => (++pulls, { done: false, value: pulls }) }; } }); const r = s.getReader(); const a = await r.read(); const b = await r.read(); return [a.value, b.value, pulls]; });
  await show('response async iterable', async () => await new Response((async function* () { yield 'a'; yield 'b'; })()).text());
  await show('response sync generator', async () => await new Response((function* () { yield 'a'; yield 'b'; })()).text());
  await show('response body is stream', async () => { const r = new Response((async function* () { yield 'a'; })()); return [r.body instanceof ReadableStream, await r.text()]; });
  await show('request async iterable', async () => await new Request('http://localhost/', { method: 'POST', body: (async function* () { yield 'q'; })() }).text());
})();
