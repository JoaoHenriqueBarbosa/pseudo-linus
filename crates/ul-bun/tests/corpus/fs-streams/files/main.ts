import fs from 'node:fs';
import fsp from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';

const base = '/tmp/s';
fs.mkdirSync(base);
const show = (...parts: unknown[]) => console.log(...parts);
const once = (emitter: any, event: string) => new Promise<any>((resolve) => emitter.once(event, (...args: any[]) => resolve(args)));

const main = async () => {
  show('classes', fs.ReadStream.name, fs.WriteStream.name, fs.FileReadStream === fs.ReadStream, typeof fs.createReadStream, fs.createReadStream.length, fs.createWriteStream.name);
  fs.writeFileSync(`${base}/in.txt`, 'abcdefghij');

  const rs = fs.createReadStream(`${base}/in.txt`, { encoding: 'utf8', highWaterMark: 4 });
  show('rs', rs instanceof fs.ReadStream, rs.constructor.name, rs.path, rs.flags, rs.mode.toString(8), rs.fd, rs.bytesRead, rs.pos, rs.start, rs.end, rs.autoClose);
  const chunks: string[] = [];
  rs.on('data', (c) => chunks.push(c as string));
  rs.on('open', (fd) => show('open event', typeof fd));
  rs.on('ready', () => show('ready event'));
  await once(rs, 'close');
  show('read chunks', chunks, rs.bytesRead, rs.destroyed, rs.fd);

  const range = fs.createReadStream(`${base}/in.txt`, { start: 2, end: 5 });
  const parts: Buffer[] = [];
  for await (const chunk of range) parts.push(chunk as Buffer);
  show('range', Buffer.concat(parts).toString());

  const missing = fs.createReadStream(`${base}/zz`);
  const [error] = await once(missing, 'error');
  show('missing', error.message, error.code, error.syscall, error.path, error.errno);

  const ws = fs.createWriteStream(`${base}/out.txt`);
  show('ws', ws instanceof fs.WriteStream, ws.constructor.name, ws.path, ws.flags, ws.mode.toString(8), ws.bytesWritten, ws.pos, ws.autoClose);
  ws.on('open', () => show('ws open'));
  ws.on('finish', () => show('ws finish'));
  const closed = once(ws, 'close');
  show('write ret', ws.write('hello '), ws.write(Buffer.from('world')));
  ws.end('!');
  await closed;
  show('written', fs.readFileSync(`${base}/out.txt`, 'utf8'), ws.bytesWritten);

  const appendStream = fs.createWriteStream(`${base}/out.txt`, { flags: 'a' });
  appendStream.end('++');
  await once(appendStream, 'close');
  show('appended', fs.readFileSync(`${base}/out.txt`, 'utf8'));

  await pipeline(fs.createReadStream(`${base}/in.txt`), fs.createWriteStream(`${base}/copy.txt`));
  show('pipeline', fs.readFileSync(`${base}/copy.txt`, 'utf8'));

  const handle = await fsp.open(`${base}/in.txt`, 'r');
  const hs = handle.createReadStream({ encoding: 'utf8' });
  const text: string[] = [];
  for await (const chunk of hs) text.push(chunk as string);
  show('handle stream', text.join(''), hs.constructor.name);
  await handle.close().catch((e: any) => show('close after stream', e.code));

  try { (fs.createReadStream as any)(5); } catch (e: any) { show('bad path', e.message, e.code); }
  try { fs.createReadStream(`${base}/in.txt`, { start: 5, end: 2 }); } catch (e: any) { show('bad range', e.message, e.code); }
  try { fs.createReadStream(`${base}/in.txt`, 'zz' as any); } catch (e: any) { show('bad enc', e.message, e.code); }
  fs.rmSync(base, { recursive: true });
};
main().then(() => show('done'));
