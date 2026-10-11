const fs = require('node:fs');
fs.mkdirSync('/tmp/ub', { recursive: true });
const fd = fs.openSync('/tmp/ub/fd.txt', 'w');
const f = new fs.Utf8Stream({ fd, sync: true, contentMode: 'buffer' });
console.log('buffer mode', f.write(Buffer.from('raw')), f.fd === fd);
try { f.write('text'); } catch (e) { console.log('buffer mode text', e.code, e.message); }
f.flushSync();
f.destroy();
console.log('destroyed', JSON.stringify(fs.readFileSync('/tmp/ub/fd.txt', 'utf8')));
