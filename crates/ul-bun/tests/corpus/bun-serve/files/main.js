const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, e.message); }
};
const sorted = (headers) => [...headers].sort((a, b) => (a[0] < b[0] ? -1 : 1));
const normalizeHost = (value) => String(value).replace(/:\d+/g, ':PORT');
const plain = (headers) => sorted(headers).map(([k, v]) => [k, k === 'date' ? 'DATE' : v]);
(async () => {
  const seen = [];
  const server = Bun.serve({
    port: 0,
    async fetch(req, srv) {
      const url = new URL(req.url);
      seen.push([req.method, url.pathname + url.search, srv === server, sorted(req.headers).map(([k, v]) => [k, normalizeHost(v)])]);
      switch (url.pathname) {
        case '/text': return new Response('hello');
        case '/empty': return new Response();
        case '/null': return new Response(null, { status: 204 });
        case '/bytes': return new Response(new Uint8Array([104, 105]));
        case '/blob': return new Response(new Blob(['blob!'], { type: 'text/x-test' }));
        case '/json': return Response.json({ a: [1, 2] });
        case '/status': return new Response('nope', { status: 404, statusText: 'Gone Fishing', headers: { 'x-a': '1', 'X-B': 'two' } });
        case '/cookies': { const h = new Headers(); h.append('set-cookie', 'a=1'); h.append('set-cookie', 'b=2'); return new Response('c', { headers: h }); }
        case '/redirect': return Response.redirect(new URL('/text', req.url), 302);
        case '/echo': return new Response(await req.text());
        case '/echo-json': return Response.json(await req.json());
        case '/stream': return new Response(new ReadableStream({ async start(c) { c.enqueue(new TextEncoder().encode('one,')); await null; c.enqueue(new TextEncoder().encode('two')); c.close(); } }));
        case '/ct': return new Response('x', { headers: { 'content-type': 'application/x-custom' } });
        default: return new Response('default ' + url.pathname, { status: 200 });
      }
    },
  });
  const base = `http://localhost:${server.port}`;
  await show('server props', async () => [typeof server.port, server.port > 0, server.hostname, server.url.href.replace(/:\d+/, ':PORT'), server.development, server.pendingRequests, server.pendingWebSockets, typeof server.stop, typeof server.reload, typeof server.id]);
  await show('text', async () => { const r = await fetch(base + '/text'); return [r.status, r.statusText, r.ok, r.url.replace(/:\d+/, ':PORT'), r.redirected, r.type, plain(r.headers), await r.text()]; });
  await show('empty', async () => { const r = await fetch(base + '/empty'); return [r.status, plain(r.headers), JSON.stringify(await r.text())]; });
  await show('null 204', async () => { const r = await fetch(base + '/null'); return [r.status, r.statusText, plain(r.headers), r.body === null]; });
  await show('bytes', async () => { const r = await fetch(base + '/bytes'); return [plain(r.headers), Array.from(await r.bytes())]; });
  await show('blob', async () => { const r = await fetch(base + '/blob'); return [r.headers.get('content-type'), r.headers.get('content-length'), await r.text()]; });
  await show('json', async () => { const r = await fetch(base + '/json'); return [r.headers.get('content-type'), await r.json()]; });
  await show('status', async () => { const r = await fetch(base + '/status'); return [r.status, r.statusText, r.ok, r.headers.get('x-a'), r.headers.get('x-b'), await r.text()]; });
  await show('cookies', async () => { const r = await fetch(base + '/cookies'); return [r.headers.getSetCookie(), await r.text()]; });
  await show('redirect follow', async () => { const r = await fetch(base + '/redirect'); return [r.status, r.redirected, r.url.replace(/:\d+/, ':PORT'), await r.text()]; });
  await show('redirect manual', async () => { const r = await fetch(base + '/redirect', { redirect: 'manual' }); return [r.status, r.redirected, r.headers.get('location').replace(/:\d+/, ':PORT'), await r.text()]; });
  await show('post echo', async () => { const r = await fetch(base + '/echo', { method: 'POST', body: 'payload' }); return await r.text(); });
  await show('post json', async () => { const r = await fetch(base + '/echo-json', { method: 'POST', body: JSON.stringify({ k: 1 }), headers: { 'content-type': 'application/json' } }); return await r.json(); });
  await show('post bytes', async () => { const r = await fetch(base + '/echo', { method: 'POST', body: new Uint8Array([65, 66]) }); return await r.text(); });
  await show('post big', async () => { const r = await fetch(base + '/echo', { method: 'POST', body: 'x'.repeat(100000) }); return (await r.text()).length; });
  await show('post stream', async () => { const r = await fetch(base + '/echo', { method: 'POST', body: new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode('s1')); c.enqueue(new TextEncoder().encode('s2')); c.close(); } }) }); return await r.text(); });
  await show('stream', async () => { const r = await fetch(base + '/stream'); return [plain(r.headers), await r.text()]; });
  await show('stream chunks', async () => { const r = await fetch(base + '/stream'); const out = []; for await (const c of r.body) out.push(new TextDecoder().decode(c)); return out; });
  await show('ct', async () => { const r = await fetch(base + '/ct'); return r.headers.get('content-type'); });
  await show('query', async () => { const r = await fetch(base + '/q?a=1&b=2'); return await r.text(); });
  await show('request class', async () => { const r = await fetch(new Request(base + '/text', { method: 'PUT', headers: { 'x-req': 'yes' } })); return await r.text(); });
  await show('head', async () => { const r = await fetch(base + '/text', { method: 'HEAD' }); return [r.status, r.headers.get('content-length'), JSON.stringify(await r.text())]; });
  await show('seen', async () => seen);
  await show('abort', async () => { const c = new AbortController(); const p = fetch(base + '/text', { signal: c.signal }); c.abort(); return await p; });
  await show('stop', async () => { await server.stop(); return [server.pendingRequests]; });
  await show('after stop', async () => await fetch(base + '/text'));
})();
