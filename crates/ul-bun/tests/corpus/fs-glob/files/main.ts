import fs from 'node:fs';
import fsp from 'node:fs/promises';

const base = '/tmp/g';
fs.mkdirSync(`${base}/src/deep/er`, { recursive: true });
fs.mkdirSync(`${base}/src/.hidden`, { recursive: true });
for (const file of ['a.txt', 'b.js', 'c.txt', '.dot', 'src/d.txt', 'src/e.ts', 'src/deep/f.txt', 'src/deep/er/g.txt', 'src/.hidden/h.txt']) {
  fs.writeFileSync(`${base}/${file}`, file);
}
fs.symlinkSync('src', `${base}/linked`);

const show = (name: string, fn: () => unknown) => {
  try {
    console.log(name, 'ok', fn());
  } catch (e: any) {
    console.log(name, 'err', e.name, e.code, e.message, JSON.stringify(Object.keys(e)));
  }
};
const opts = { cwd: base };

show('star', () => fs.globSync('*', opts));
show('txt', () => fs.globSync('*.txt', opts));
show('globstar', () => fs.globSync('**/*.txt', opts));
show('subdir', () => fs.globSync('src/*', opts));
show('brace', () => fs.globSync('*.{js,txt}', opts));
show('class', () => fs.globSync('[ab].*', opts));
show('question', () => fs.globSync('?.txt', opts));
show('multi', () => fs.globSync(['*.js', 'src/*.ts'], opts));
show('absolute', () => fs.globSync(`${base}/src/*.txt`));
show('none', () => fs.globSync('nothing/*', opts));
show('dot', () => fs.globSync('**/*', { cwd: `${base}/src` }));
show('exclude fn', () => fs.globSync('**/*.txt', { cwd: base, exclude: (name: string) => name.startsWith('src/deep') }));
show('exclude array', () => fs.globSync('**/*.txt', { cwd: base, exclude: ['src/deep/**'] }));
show('withFileTypes', () => fs.globSync('src/*', { cwd: base, withFileTypes: true }).map((entry: any) => [entry.constructor.name, entry.name, entry.parentPath, entry.isDirectory()]));
show('cwd url', () => fs.globSync('*.js', { cwd: new URL(`file://${base}/`) }));
show('bad pattern', () => (fs.globSync as any)(5));
show('bad options', () => (fs.globSync as any)('*', 5));
show('bad exclude', () => (fs.globSync as any)('*', { exclude: 5 }));
show('lengths', () => [fs.glob.length, fs.globSync.length, fsp.glob.length, fs.glob.name, fs.globSync.name, fsp.glob.name]);

(async () => {
  const collected: string[] = [];
  for await (const entry of fsp.glob('**/*.txt', opts)) collected.push(entry as string);
  console.log('promises', collected);
  console.log('promises type', Object.prototype.toString.call(fsp.glob('*', opts)));
  try {
    for await (const entry of (fsp.glob as any)(5)) console.log(entry);
  } catch (e: any) {
    console.log('promises bad', e.code, e.message);
  }
  await new Promise<void>((resolve) => {
    fs.glob('src/*.txt', opts, (...args: unknown[]) => {
      console.log('cb', args);
      resolve();
    });
  });
  await new Promise<void>((resolve) => {
    fs.glob('*.js', (...args: unknown[]) => {
      console.log('cb cwd', args.length, (args[1] as string[]).length > 0);
      resolve();
    });
  });
  try { (fs.glob as any)('*', opts); } catch (e: any) { console.log('cb missing', e.code, e.message); }
  fs.rmSync(base, { recursive: true });
})();
