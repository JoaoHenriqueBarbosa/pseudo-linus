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
  await show('break cancels', async () => { let cancelled = null; const s = new ReadableStream({ pull(c) { c.enqueue(1); }, cancel(r) { cancelled = r; } }); for await (const c of s) break; return [cancelled, s.locked]; });
  await show('prevent cancel', async () => { let cancelled = false; const s = new ReadableStream({ pull(c) { c.enqueue(1); }, cancel() { cancelled = true; } }); for await (const c of s.values({ preventCancel: true })) break; return [cancelled, s.locked]; });
  await show('locked', async () => { const s = new ReadableStream(); s.getReader(); return s.values(); });
  await show('errored', async () => { const s = new ReadableStream({ start(c) { c.error(new Error('boom')); } }); const out = []; for await (const c of s) out.push(c); return out; });
  await show('after done', async () => { const s = new ReadableStream({ start(c) { c.close(); } }); const it = s.values(); const a = await it.next(); const b = await it.next(); const r = await it.return(7); return [a, b, r, s.locked]; });
  await show('return value', async () => { const s = new ReadableStream({ start(c) { c.enqueue(1); } }); const it = s.values(); const r = await it.return(9); return [r, s.locked]; });
  await show('chain', async () => { const s = new ReadableStream({ start(c) { c.enqueue(1); c.enqueue(2); c.close(); } }); const it = s.values(); const all = await Promise.all([it.next(), it.next(), it.next()]); return all; });
  await show('proto', async () => { const it = new ReadableStream().values(); const p = Object.getPrototypeOf(it); const ap = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}.prototype)); return [Object.getPrototypeOf(p) === ap, Object.getOwnPropertyNames(p), it[Symbol.asyncIterator]() === it, Object.prototype.toString.call(it)]; });
  await show('bad this', async () => { const it = new ReadableStream().values(); return await Object.getPrototypeOf(it).next.call({}); });
})();
