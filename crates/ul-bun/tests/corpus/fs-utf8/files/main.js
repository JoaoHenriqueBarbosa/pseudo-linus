const fs = require('node:fs');
const fsp = fs.promises;
fs.mkdirSync('/tmp/u', { recursive: true });
fs.writeFileSync('/tmp/u/x', 'a');
const show = (name, fn) => {
  try { console.log(name, 'ok', fn()); } catch (e) { console.log(name, 'err', e.name, e.code, e.message, Object.keys(e)); }
};
(async () => {
  console.log('lchmod', typeof fsp.lchmod, fsp.lchmod.length, fsp.lchmod.name, Object.keys(fsp).indexOf('lchmod'));
  for (const args of [['/tmp/u/x', 0o600], ['/tmp/zz', 0o600], []]) {
    try { console.log('p lchmod ok', await fsp.lchmod(...args)); } catch (e) { console.log('p lchmod err', e.name, e.code, e.message, Object.keys(e), e.syscall, e.path); }
  }
  console.log('fs lchmod', typeof fs.lchmod, 'lchmod' in fs, typeof fs.lchmodSync, Object.getOwnPropertyDescriptor(fs, 'lchmod'));

  const U = fs.Utf8Stream;
  console.log('class', typeof U, U.name, U.length, Object.getOwnPropertyNames(U.prototype).join(), Object.getOwnPropertyNames(U).join());
  console.log('same', fs.Utf8Stream === U, Object.getOwnPropertyDescriptor(fs, 'Utf8Stream').writable);
  const s = new U({ dest: '/tmp/u/out.txt', sync: true });
  console.log('instance', s.constructor.name, Object.keys(s).join(), s.file, s.sync, s.contentMode, s.mode, s.minLength, s.maxLength, s.append, s.writing);
  console.log('write', s.write('hello '), s.write('wörld'));
  s.end();
  await new Promise((resolve) => s.on('close', resolve));
  console.log('closed', JSON.stringify(fs.readFileSync('/tmp/u/out.txt', 'utf8')));

  const a = new U({ dest: '/tmp/u/async.txt', minLength: 0 });
  a.on('ready', () => console.log('ready'));
  console.log('async write', a.write('line 1\n'), a.write('line 2\n'));
  a.end();
  await new Promise((resolve) => a.on('close', resolve));
  console.log('async closed', JSON.stringify(fs.readFileSync('/tmp/u/async.txt', 'utf8')));

  const fd = fs.openSync('/tmp/u/fd.txt', 'w');
  const f = new U({ fd, sync: true });
  console.log('fd mode', f.write('on a descriptor'), f.fd === fd);
  f.flushSync();
  f.destroy();
  console.log('destroyed', JSON.stringify(fs.readFileSync('/tmp/u/fd.txt', 'utf8')));
  try { f.write('x'); } catch (e) { console.log('after destroy', e.code, e.name, e.message); }

  show('no args', () => new U());
  show('no dest', () => new U({}));
  show('bad fd', () => new U({ fd: {} }));
  show('bad min', () => new U({ fd: 1, minLength: 5000000, maxWrite: 10 }));
  show('bad mode', () => new U({ fd: 1, contentMode: 'x' }));
  show('bad sync', () => new U({ fd: 1, sync: 5 }));
  fs.rmSync('/tmp/u', { recursive: true });
})();
