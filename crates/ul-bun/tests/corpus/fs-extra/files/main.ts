import fs from 'node:fs';
const fsp = fs.promises;
const base = '/tmp/x';
fs.mkdirSync(base);
fs.writeFileSync(`${base}/a`, 'hello');
const t = (name: string, fn: () => unknown) => {
  try { console.log(name, 'ok', fn()); } catch (e: any) { console.log(name, 'err', e.message, JSON.stringify(Object.keys(e)), e.code, e.syscall, e.errno, e.name); }
};
t('chown', () => fs.chownSync(`${base}/a`, 0, 0));
t('chown missing', () => fs.chownSync(`${base}/zz`, 0, 0));
t('lchown', () => fs.lchownSync(`${base}/a`, 0, 0));
t('fchown', () => { const fd = fs.openSync(`${base}/a`, 'r'); const r = fs.fchownSync(fd, 0, 0); fs.closeSync(fd); return r; });
t('chown unchanged', () => fs.chownSync(`${base}/a`, -1, -1));
t('chown args', () => (fs.chownSync as any)(`${base}/a`));
t('lchmod', () => [typeof (fs as any).lchmod, typeof (fs as any).lchmodSync, Object.keys(fs).includes('lchmod')]);
t('statfs', () => { const s = fs.statfsSync('/tmp'); return [s.constructor.name, Object.keys(s).join(), typeof s.bsize, typeof s.blocks, s.bsize > 0]; });
t('statfs big', () => { const s = fs.statfsSync('/tmp', { bigint: true }); return [s.constructor.name, typeof s.bsize]; });
t('statfs missing', () => fs.statfsSync(`${base}/zz`));
t('writev', () => { const fd = fs.openSync(`${base}/w`, 'w'); const r = fs.writevSync(fd, [Buffer.from('ab'), Buffer.from('cde')]); fs.closeSync(fd); return [r, fs.readFileSync(`${base}/w`, 'utf8')]; });
t('readv', () => { const fd = fs.openSync(`${base}/w`, 'r'); const a = Buffer.alloc(2), b = Buffer.alloc(5); const r = fs.readvSync(fd, [a, b], 0); fs.closeSync(fd); return [r, a, b]; });
t('writev bad', () => (fs.writevSync as any)(1, 'x'));
t('mkdtempDisposable', () => { const d = fs.mkdtempDisposableSync(`${base}/d-`); const keys = Object.keys(d); const had = fs.existsSync(d.path); d.remove(); return [keys, had, fs.existsSync(d.path), typeof (d as any)[Symbol.dispose], d.path.length]; });
t('opendir', () => { const d = fs.opendirSync(base); const names: string[] = []; let e; while ((e = d.readSync())) names.push(e.name); d.closeSync(); return [d.constructor.name, d.path, names.sort(), Object.keys(d)]; });
t('opendir missing', () => fs.opendirSync(`${base}/zz`));
t('opendir file', () => fs.opendirSync(`${base}/a`));
t('Dir', () => [typeof fs.Dir, fs.Dir.name, Object.getOwnPropertyNames(fs.Dir.prototype)]);
t('openAsBlob', () => [fs.openAsBlob(`${base}/a`) instanceof Promise]);
t('lengths', () => [fs.chown.length, fs.statfs.length, fs.opendir.length, fs.writev.length, fs.chownSync.length, fs.statfsSync.length, fsp.statfs.length, fsp.writev.length, fsp.mkdtempDisposable.length, fs.openAsBlob.length]);

(async () => {
  console.log('p chown', await fsp.chown(`${base}/a`, 0, 0));
  console.log('p statfs', Object.keys(await fsp.statfs('/tmp')).join());
  const h = await fsp.open(`${base}/a`);
  console.log('h chown', await h.chown(0, 0));
  console.log('h readv', await h.readv([Buffer.alloc(2)], 0));
  await h.close();
  const d = await fsp.opendir(base);
  const names: string[] = [];
  for await (const e of d) names.push(e.name);
  console.log('p opendir', names.sort());
  const md = await fsp.mkdtempDisposable(`${base}/e-`);
  console.log('p mkdtempDisposable', Object.keys(md), fs.existsSync(md.path), await md.remove(), fs.existsSync(md.path));
  const blob = await fs.openAsBlob(`${base}/a`);
  console.log('blob', blob.constructor.name, blob.size, await blob.text());
  fs.rmSync(base, { recursive: true });
})();
