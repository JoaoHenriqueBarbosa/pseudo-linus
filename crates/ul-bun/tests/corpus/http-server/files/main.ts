const server = Bun.serve({ port: 0, fetch(req: Request) { return new Response('hi ' + new URL(req.url).pathname); } });
const res = await fetch(`http://localhost:${server.port}/abc`);
console.log(res.status, await res.text());
server.stop();
