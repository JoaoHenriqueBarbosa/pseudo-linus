const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, String(e.message).replace(/:\d+/g, ':PORT')); }
};
(async () => {
  const server = Bun.serve({
    port: 0,
    fetch(req) {
      const path = new URL(req.url).pathname;
      if (path === '/redirect') return Response.redirect(new URL('/text', req.url), 302);
      return new Response('text');
    },
  });
  const base = `http://localhost:${server.port}`;
  await show('redirect error', async () => await fetch(base + '/redirect', { redirect: 'error' }));
  await show('redirect follow off', async () => (await fetch(base + '/redirect', { redirect: 'manual' })).status);
  await server.stop();
  await show('refused', async () => await fetch(base + '/x'));
  await show('refused localhost closed port', async () => await fetch('http://localhost:1/x'));
  await show('refused 127', async () => await fetch('http://127.0.0.1:1/x'));
})();
