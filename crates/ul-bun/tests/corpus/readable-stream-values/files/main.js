const show = async (name, fn) => {
  try { console.log(name, 'ok', await fn()); } catch (e) { console.log(name, 'err', e.name, e.code, e.message); }
};
(async () => {
  await show('blob iter', async () => { const out = []; for await (const c of new Blob(['abc']).stream()) out.push(c.length); return out; });
  await show('blob reader', async () => { const r = new Blob(['abc']).stream().getReader(); const a = await r.read(); const b = await r.read(); return [a.value.length, a.done, b.done]; });
  await show('response iter', async () => { const out = []; for await (const c of new Response('abc').body) out.push(c.length); return out; });
  await show('stream iter', async () => { const out = []; for await (const c of new ReadableStream({ start(c) { c.enqueue(1); c.close(); } })) out.push(c); return out; });
  await show('values', async () => { const s = new Blob(['abc']).stream(); return [typeof s[Symbol.asyncIterator], s[Symbol.asyncIterator] === s.values, s.constructor.name]; });
  await show('values call', async () => { const s = new Blob(['abc']).stream(); const it = s.values(); const r = await it.next(); return [r.done, r.value.length]; });
})();
