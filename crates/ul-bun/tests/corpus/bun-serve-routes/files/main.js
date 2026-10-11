const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, String(e.message).replace(/:\d+/g, ':PORT')); }
};
const sorted = (headers) => [...headers].sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(([k, v]) => [k, k === 'date' ? 'DATE' : v]);
(async () => {
  const server = Bun.serve({
    port: 0,
    development: false,
    routes: {
      '/static': new Response('static body', { headers: { 'x-static': 'yes' } }),
      '/users/:id': (req) => new Response('user ' + req.params.id),
      '/files/*': (req) => new Response('file ' + new URL(req.url).pathname),
      '/methods': { GET: () => new Response('got'), POST: async (req) => new Response('posted ' + (await req.text())) },
    },
    fetch(req) {
      const path = new URL(req.url).pathname;
      if (path === '/throw') throw new Error('boom');
      if (path === '/reject') return Promise.reject(new Error('rejected'));
      return new Response('fallback ' + path, { status: 404 });
    },
    error(err) {
      return new Response('handled ' + err.message, { status: 500, headers: { 'content-type': 'text/x-error' } });
    },
  });
  const base = `http://localhost:${server.port}`;
  const get = async (path, init) => { const r = await fetch(base + path, init); return [r.status, r.statusText, r.headers.get('content-type'), r.headers.get('x-static'), await r.text()]; };
  await show('static', () => get('/static'));
  await show('param', () => get('/users/42'));
  await show('param encoded', () => get('/users/a%20b'));
  await show('wildcard', () => get('/files/a/b/c.txt'));
  await show('method get', () => get('/methods'));
  await show('method post', () => get('/methods', { method: 'POST', body: 'data' }));
  await show('method not allowed', () => get('/methods', { method: 'DELETE' }));
  await show('fallback', () => get('/nothing'));
  await show('throw', () => get('/throw'));
  await show('reject', () => get('/reject'));
  await show('props', async () => [server.development, server.hostname, server.pendingRequests]);
  await show('reload', async () => { server.reload({ fetch: () => new Response('reloaded') }); return await get('/nothing'); });
  await show('direct fetch', async () => { const r = await server.fetch(new Request('http://localhost/direct')); return [r.status, await r.text()]; });
  await server.stop();
})();
