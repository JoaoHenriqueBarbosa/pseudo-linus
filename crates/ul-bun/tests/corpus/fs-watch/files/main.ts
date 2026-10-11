import fs from 'node:fs';
import fsp from 'node:fs/promises';

const base = '/tmp/w';
fs.mkdirSync(base, { recursive: true });
fs.writeFileSync(`${base}/f`, 'one');
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const kinds = (log: string[]) => [...new Set(log)].sort();

(async () => {
  const w = fs.watch(`${base}/f`);
  console.log('shape', w.constructor.name, Object.keys(w), (w as any)._handle.constructor.name, Object.getOwnPropertyNames(Object.getPrototypeOf(w)), w.ref() === w, w.unref() === w);
  console.log('lengths', fs.watch.length, fs.watchFile.length, fs.unwatchFile.length, fsp.watch.length, fs.watch.name, fs.watchFile.name, fs.unwatchFile.name, fsp.watch.name);
  const closed = new Promise<void>((resolve) => w.on('close', () => resolve()));
  w.close();
  w.close();
  await closed;
  console.log('closed after close()');

  try { fs.watch(`${base}/zz`); } catch (e: any) { console.log('missing', e.name, e.code, e.message, JSON.stringify(Object.keys(e)), e.syscall, e.errno, e.path, e.filename); }
  console.log('throwIfNoEntry', fs.watch(`${base}/zz`, { throwIfNoEntry: false }).constructor.name);

  const fileLog: string[] = [];
  const fw = fs.watch(`${base}/f`, (type, name) => fileLog.push(`${type} ${name}`));
  await sleep(150);
  fs.writeFileSync(`${base}/f`, 'changed content');
  await sleep(400);
  fw.close();
  console.log('file events', kinds(fileLog));

  const dirLog: string[] = [];
  const dw = fs.watch(base, { encoding: 'utf8' }, (type, name) => dirLog.push(`${type} ${name}`));
  await sleep(150);
  fs.writeFileSync(`${base}/n`, 'x');
  await sleep(400);
  fs.unlinkSync(`${base}/n`);
  await sleep(400);
  dw.close();
  console.log('dir events', kinds(dirLog).filter((entry) => entry.endsWith(' n')).filter((entry) => entry.startsWith('rename')));

  const bufferLog: unknown[] = [];
  const bw = fs.watch(base, { encoding: 'buffer' }, (type, name) => bufferLog.push(Buffer.isBuffer(name)));
  await sleep(150);
  fs.writeFileSync(`${base}/b`, 'x');
  await sleep(400);
  bw.close();
  console.log('buffer names', [...new Set(bufferLog)]);

  for (const make of [() => AbortSignal.abort(), () => { const ac = new AbortController(); setTimeout(() => ac.abort(), 30); return ac.signal; }]) {
    const watcher = fs.watch(base, { signal: make() });
    await new Promise<void>((resolve) => {
      watcher.on('error', (e: any) => console.log('abort error', e.name));
      watcher.on('close', () => { console.log('abort close'); resolve(); });
    });
  }

  const ac = new AbortController();
  setTimeout(() => ac.abort(), 60);
  try {
    for await (const event of fsp.watch(base, { signal: ac.signal })) console.log('p event', event);
  } catch (e: any) { console.log('p watch', e.name, e.code); }
  try {
    for await (const event of fsp.watch(base, { signal: AbortSignal.abort() })) console.log('p event', event);
  } catch (e: any) { console.log('p aborted', e.name, e.code); }
  try { fsp.watch(5 as any); } catch (e: any) { console.log('p bad', e.code, e.message); }
  try { fsp.watch(`${base}/zz`)[Symbol.asyncIterator](); console.log('p lazy ok'); } catch (e: any) { console.log('p missing', e.code); }
  try { for await (const event of fsp.watch(`${base}/zz`)) console.log(event); } catch (e: any) { console.log('p missing iterate', e.code, e.message); }

  const changes: unknown[] = [];
  fs.writeFileSync(`${base}/s`, 'abc');
  const sw = fs.watchFile(`${base}/s`, { interval: 40 }, (curr, prev) => changes.push([curr.size, prev.size, curr.constructor.name]));
  console.log('stat watcher', sw.constructor.name, Object.keys(sw), Object.getOwnPropertyNames(Object.getPrototypeOf(sw)), sw.listenerCount('change'), fs.watchFile(`${base}/s`, () => {}) === sw, sw.listenerCount('change'));
  await sleep(150);
  fs.writeFileSync(`${base}/s`, 'abcdef');
  await sleep(300);
  fs.unlinkSync(`${base}/s`);
  await sleep(300);
  fs.unwatchFile(`${base}/s`);
  console.log('watchFile', changes);
  fs.unwatchFile(`${base}/never-watched`);

  await new Promise<void>((resolve) => {
    fs.watchFile(`${base}/never`, { interval: 30, bigint: true }, (curr, prev) => {
      console.log('missing watchFile', curr.constructor.name, curr.size, prev.size, curr.mtimeMs, curr.ino);
      fs.unwatchFile(`${base}/never`);
      resolve();
    });
  });
  try { (fs.watchFile as any)(`${base}/s`, {}, 5); } catch (e: any) { console.log('bad listener', e.code, e.message); }
  try { (fs.watchFile as any)(5, () => {}); } catch (e: any) { console.log('bad path', e.code, e.message); }
  fs.rmSync(base, { recursive: true });
})();
