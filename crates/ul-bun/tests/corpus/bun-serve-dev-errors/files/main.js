const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, String(e.message).replace(/:\d+/g, ':PORT')); }
};
(async () => {
  const server = Bun.serve({
    port: 0,
    fetch(req) {
      const path = new URL(req.url).pathname;
      if (path === '/throw') throw new Error('handler boom');
      if (path === '/reject') return Promise.reject(new Error('handler reject'));
      if (path === '/undefined') return undefined;
      return new Response('ok');
    },
  });
  const base = `http://localhost:${server.port}`;
  for (const path of ['/throw', '/reject', '/undefined']) {
    await show(path, async () => { const r = await fetch(base + path); const text = await r.text(); return [r.status, r.statusText, r.headers.get('content-type'), text.length > 100 ? 'long' : text]; });
  }
  await server.stop();
})();
