import fs from 'node:fs';
import fsp from 'node:fs/promises';
import { promisify } from 'node:util';

const base = '/tmp/a';
const show = (...parts: unknown[]) => console.log(...parts);
const call = <T>(fn: (cb: (err: any, ...rest: any[]) => void) => void): Promise<any[]> => new Promise((resolve) => fn((...args) => resolve(args)));

fs.mkdirSync(base);
const checks = async () => {
  show('same', fsp === fs.promises, typeof fs.promises.readFile);
  for (const [name, fn] of [['nocb', () => (fs.readFile as any)('/app/main.ts')], ['badcb', () => (fs.readFile as any)('/app/main.ts', 'utf8', 5)], ['stat noargs', () => (fs.stat as any)()]] as const) {
    try { fn(); } catch (e: any) { show(name, e.message, e.code, e.name); }
  }
  show('lengths', [fs.readFile.length, fsp.readFile.length, fsp.readFile.name, fs.exists.length, fsp.exists.length, fs.read.length]);

  let r = await call((cb) => fs.readFile('/nope', cb));
  show('cb err', r[0].message, Object.keys(r[0]), r.length, r[1]);
  r = await call((cb) => fs.readFile('/app/main.ts', 'utf8', cb));
  show('cb ok', r[0], typeof r[1], r.length);
  r = await call((cb) => fs.stat('/app/main.ts', cb));
  show('cb stat', r[0], r[1].constructor.name, r[1].isFile());
  r = await call((cb) => fs.writeFile(`${base}/w`, 'abc', cb));
  show('cb writeFile', r);
  r = await call((cb) => fs.appendFile(`${base}/w`, 'def', cb));
  show('cb appendFile', r, fs.readFileSync(`${base}/w`, 'utf8'));
  r = await call((cb) => fs.mkdir(`${base}/x/y`, { recursive: true }, cb));
  show('cb mkdir', r);
  r = await call((cb) => fs.readdir(base, cb));
  show('cb readdir', r[0], r[1].sort());
  r = await call((cb) => fs.exists(`${base}/w`, cb));
  show('cb exists', r);
  r = await call((cb) => fs.access('/nope', cb));
  show('cb access', r[0].code);
  r = await call((cb) => fs.rename(`${base}/w`, `${base}/w2`, cb));
  show('cb rename', r, fs.existsSync(`${base}/w2`));
  r = await call((cb) => fs.realpath('/app/../app', cb));
  show('cb realpath', r);
  r = await call((cb) => fs.open(`${base}/w2`, 'r', cb));
  show('cb open', r[0], typeof r[1]);
  const fd = r[1];
  const buffer = Buffer.alloc(4);
  r = await call((cb) => fs.read(fd, buffer, 0, 4, 0, cb));
  show('cb read', r[0], r[1], r[2] === buffer, buffer.toString());
  r = await call((cb) => fs.close(fd, cb));
  show('cb close', r);
  r = await call((cb) => fs.unlink(`${base}/w2`, cb));
  show('cb unlink', r);
  r = await call((cb) => fs.rm(`${base}/x`, { recursive: true }, cb));
  show('cb rm', r);

  show('p readFile', typeof (await fsp.readFile('/app/main.ts', 'utf8')));
  try { await fsp.readFile('/nope'); } catch (e: any) { show('p err', e.message, Object.keys(e), e.name, e.code); }
  show('p stat', (await fsp.stat('/app')).isDirectory());
  show('p writeFile', await fsp.writeFile(`${base}/p`, 'hi'));
  show('p appendFile', await fsp.appendFile(`${base}/p`, '!'), await fsp.readFile(`${base}/p`, 'utf8'));
  show('p mkdir', await fsp.mkdir(`${base}/m1/m2`, { recursive: true }));
  show('p readdir', (await fsp.readdir(base, { withFileTypes: true })).map((d) => [d.name, d.isDirectory()]).sort());
  try { await fsp.access('/nope'); } catch (e: any) { show('p access', e.code); }
  show('p rm force', await fsp.rm('/nope', { force: true }));
  show('p copyFile', await fsp.copyFile(`${base}/p`, `${base}/q`), await fsp.readFile(`${base}/q`, 'utf8'));
  show('p rename', await fsp.rename(`${base}/q`, `${base}/r`), (await fsp.readdir(base)).sort());
  show('p exists', await fsp.exists?.(`${base}/r`));
  show('promisify', await promisify(fs.readFile)(`${base}/r`, 'utf8'));

  const handle = await fsp.open(`${base}/p`, 'r+');
  show('handle', handle.constructor.name, typeof handle.fd);
  show('h readFile', await handle.readFile('utf8'));
  show('h stat', (await handle.stat()).size);
  const out = await handle.read(Buffer.alloc(3), 0, 3, 0);
  show('h read', out.bytesRead, out.buffer.toString());
  const wrote = await handle.write('Z', 0);
  show('h write', wrote.bytesWritten, wrote.buffer);
  await handle.close();
  show('closed read', await fsp.readFile(`${base}/p`, 'utf8'));
  try { await handle.stat(); } catch (e: any) { show('h after close', e.code); }
  await fsp.rm(base, { recursive: true });
};
checks().then(() => show('done'));
