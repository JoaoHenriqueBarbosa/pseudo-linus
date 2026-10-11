import fs from 'node:fs';
import fsp from 'node:fs/promises';

const base = '/tmp/c';
fs.mkdirSync(`${base}/src/sub`, { recursive: true });
fs.writeFileSync(`${base}/src/a.txt`, 'alpha');
fs.writeFileSync(`${base}/src/sub/b.txt`, 'beta');
fs.symlinkSync('a.txt', `${base}/src/link`);

const show = (name: string, fn: () => unknown) => {
  try {
    console.log(name, 'ok', fn());
  } catch (e: any) {
    console.log(name, 'err', e.name, e.code, e.message, JSON.stringify(Object.keys(e)), e.syscall, e.errno, e.path, e.dest);
  }
};
const tree = (dir: string): string[] =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = `${dir}/${entry.name}`;
    return entry.isDirectory() ? [`${entry.name}/`, ...tree(full).map((child) => `${entry.name}/${child}`)] : [entry.name];
  }).sort();

show('file', () => { fs.cpSync(`${base}/src/a.txt`, `${base}/out-file`); return fs.readFileSync(`${base}/out-file`, 'utf8'); });
show('file into new dir', () => { fs.cpSync(`${base}/src/a.txt`, `${base}/deep/er/a.txt`); return fs.readFileSync(`${base}/deep/er/a.txt`, 'utf8'); });
show('dir not recursive', () => fs.cpSync(`${base}/src`, `${base}/out-dir`));
show('dir recursive', () => { fs.cpSync(`${base}/src`, `${base}/out-dir`, { recursive: true }); return [tree(`${base}/out-dir`), fs.readlinkSync(`${base}/out-dir/link`)]; });
show('dir merge', () => { fs.writeFileSync(`${base}/out-dir/extra`, 'x'); fs.cpSync(`${base}/src`, `${base}/out-dir`, { recursive: true }); return tree(`${base}/out-dir`); });
show('dereference', () => { fs.cpSync(`${base}/src`, `${base}/out-deref`, { recursive: true, dereference: true }); return fs.lstatSync(`${base}/out-deref/link`).isSymbolicLink(); });
show('errorOnExist', () => fs.cpSync(`${base}/src/a.txt`, `${base}/out-file`, { errorOnExist: true, force: false }));
show('force false keeps', () => { fs.writeFileSync(`${base}/keep`, 'old'); fs.cpSync(`${base}/src/a.txt`, `${base}/keep`, { force: false }); return fs.readFileSync(`${base}/keep`, 'utf8'); });
show('same file', () => fs.cpSync(`${base}/src/a.txt`, `${base}/src/a.txt`));
show('into itself', () => fs.cpSync(`${base}/src`, `${base}/src/sub/in`, { recursive: true }));
show('dir to file', () => fs.cpSync(`${base}/src`, `${base}/out-file`, { recursive: true }));
show('file to dir', () => fs.cpSync(`${base}/src/a.txt`, `${base}/out-dir`));
show('missing', () => fs.cpSync(`${base}/zz`, `${base}/out-zz`));
show('filter', () => { fs.cpSync(`${base}/src`, `${base}/out-filter`, { recursive: true, filter: (s: string) => !s.endsWith('b.txt') }); return tree(`${base}/out-filter`); });
show('filter promise', () => fs.cpSync(`${base}/src/a.txt`, `${base}/out-fp`, { filter: (() => Promise.resolve(true)) as any }));
show('incompatible', () => fs.cpSync(`${base}/src`, `${base}/o`, { dereference: true, verbatimSymlinks: true }));
show('bad mode', () => fs.cpSync(`${base}/src/a.txt`, `${base}/o`, { mode: 9 }));
show('bad option type', () => fs.cpSync(`${base}/src/a.txt`, `${base}/o`, { recursive: 'yes' } as any));
show('bad src', () => (fs.cpSync as any)(5, `${base}/o`));
show('bad dest url', () => (fs.cpSync as any)(`${base}/src/a.txt`, {}));
show('null byte', () => fs.cpSync(`${base}/src/a.txt\u0000x`, `${base}/o`));
show('preserveTimestamps', () => { fs.utimesSync(`${base}/src/a.txt`, 1000, 2000); fs.cpSync(`${base}/src/a.txt`, `${base}/out-ts`, { preserveTimestamps: true }); return fs.statSync(`${base}/out-ts`).mtimeMs; });
show('lengths', () => [fs.cp.length, fs.cpSync.length, fsp.cp.length, fs.cp.name, fs.cpSync.name, fsp.cp.name]);

(async () => {
  console.log('p file', await fsp.cp(`${base}/src/a.txt`, `${base}/p-file`), fs.readFileSync(`${base}/p-file`, 'utf8'));
  console.log('p dir', await fsp.cp(`${base}/src`, `${base}/p-dir`, { recursive: true }), tree(`${base}/p-dir`));
  for (const [name, run] of [
    ['p dir not recursive', () => fsp.cp(`${base}/src`, `${base}/p-x`)],
    ['p errorOnExist', () => fsp.cp(`${base}/src/a.txt`, `${base}/p-file`, { errorOnExist: true, force: false })],
    ['p missing', () => fsp.cp(`${base}/zz`, `${base}/p-zz`)],
    ['p bad option', () => fsp.cp(`${base}/src`, `${base}/p-zz`, 5 as any)],
    ['p filter', () => fsp.cp(`${base}/src`, `${base}/p-filter`, { recursive: true, filter: async (s: string) => !s.endsWith('sub') })],
  ] as [string, () => Promise<unknown>][]) {
    try {
      console.log(name, 'ok', await run());
    } catch (e: any) {
      console.log(name, 'err', e.name, e.code, e.message, JSON.stringify(Object.keys(e)));
    }
  }
  console.log('p filter tree', tree(`${base}/p-filter`));
  await new Promise<void>((resolve) => {
    fs.cp(`${base}/src`, `${base}/cb-dir`, { recursive: true }, (...args: unknown[]) => {
      console.log('cb', args, tree(`${base}/cb-dir`));
      resolve();
    });
  });
  await new Promise<void>((resolve) => {
    fs.cp(`${base}/zz`, `${base}/cb-zz`, (error: any, value) => {
      console.log('cb err', error.code, error.syscall, value);
      resolve();
    });
  });
  try { (fs.cp as any)(`${base}/src`, `${base}/cb-x`); } catch (e: any) { console.log('cb missing', e.code, e.message); }
  try { (fs.cp as any)(`${base}/src`, `${base}/cb-x`, { recursive: 1 }, () => {}); } catch (e: any) { console.log('cb bad option', e.code, e.message); }
  fs.rmSync(base, { recursive: true });
})();
