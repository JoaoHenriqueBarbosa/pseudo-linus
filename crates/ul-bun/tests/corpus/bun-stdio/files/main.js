const show = async (name, fn) => {
  try { console.log(name, 'ok', JSON.stringify(await fn())); } catch (e) { console.log(name, 'err', e.name, e.code, e.message); }
};
(async () => {
  await show('keys', async () => [typeof Bun.stdin, typeof Bun.stdout, typeof Bun.stderr, Object.getOwnPropertyNames(Bun).filter(k => /^std/.test(k))]);
  await show('shape', async () => [Bun.stdout.constructor.name, Bun.stdout instanceof Blob, Bun.stdin instanceof Blob, Bun.stdout.name, Bun.stderr.name, Bun.stdin.name, Bun.stdout.type, Bun.stdin.size, Bun.stdout.size]);
  await show('file fd shape', async () => [String(Bun.file(1).size), String(Bun.file(0).size), String(Bun.stdout.size), String(Bun.stdin.size), Bun.file(1).name, Bun.file(1).type, Bun.inspect(Bun.file(1)), Object.prototype.toString.call(Bun.stdout)]);
  await show('same object', async () => [Bun.stdout === Bun.stdout, Bun.stdin === Bun.stdin, Bun.stdout === Bun.stderr]);
  await show('write string', async () => await Bun.write(Bun.stdout, 'out-string\n'));
  await show('write bytes', async () => await Bun.write(Bun.stdout, new TextEncoder().encode('out-bytes\n')));
  await show('write blob', async () => await Bun.write(Bun.stdout, new Blob(['out-blob\n'])));
  await show('write stderr', async () => await Bun.write(Bun.stderr, 'err-string\n'));
  await show('write response', async () => await Bun.write(Bun.stdout, new Response('out-response\n')));
  await show('write empty', async () => await Bun.write(Bun.stdout, ''));
  await show('write fd', async () => await Bun.write(Bun.file(1), 'via-file-1\n'));
  await show('writer', async () => { const w = Bun.stdout.writer(); const a = w.write('w1 '); const b = w.write('w2\n'); const f = await w.flush(); await w.end(); return [typeof a, typeof b, typeof f, w.constructor.name]; });
  await show('writer stderr', async () => { const w = Bun.stderr.writer(); w.write('writer-err\n'); await w.end(); return 1; });
  await show('stat', async () => { const s = await Bun.stdout.stat(); return typeof s.size; });
  await show('exists', async () => [await Bun.stdout.exists(), await Bun.stdin.exists()]);
  await show('inspect', async () => [Bun.inspect(Bun.stdout), Bun.inspect(Bun.stdin), Bun.inspect(Bun.stderr)]);
  await show('text stdin', async () => JSON.stringify(await Bun.stdin.text()));
  await show('stdin again', async () => JSON.stringify(await Bun.stdin.text()));
  await show('stdin stream', async () => { const out = []; for await (const c of Bun.stdin.stream()) out.push(c.length); return out; });
})();
