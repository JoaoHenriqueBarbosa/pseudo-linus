# Buffer global (bun 1.4.2): medição e plano

Medido com `/tmp/buf-probe-a7c1b.js` e `/tmp/buf-probe-a7c2.js` no bun 1.4.2.

## Forma

- Global `Buffer`: dados, `writable` `enumerable` `configurable`. `Object.getPrototypeOf(Buffer) === Uint8Array`,
  `Object.getPrototypeOf(Buffer.prototype) === Uint8Array.prototype`, `Buffer.prototype.constructor === Buffer`.
- `Buffer.length` 3, `Buffer.name` "Buffer". `Buffer(3)` e `new Buffer(2)` funcionam.
- `Symbol.species` NÃO é própria do construtor; é própria de `Buffer.prototype`: não gravável, enumerável, não
  configurável, valor `Buffer` (função), a leitura `Buffer[Symbol.species] === Buffer` vem da herança do `%TypedArray%`.
- `Buffer.prototype[Symbol.toStringTag]`: valor "Uint8Array", não gravável, não enumerável, configurável.
  `Object.prototype.toString.call(buf)` dá `[object Uint8Array]`.
- `Buffer.prototype[Symbol.for('nodejs.util.inspect.custom')]` = `inspect` (length 2); `util.inspect(buf)` é
  `<Buffer 61 62>`.

## Chaves estáticas, na ordem do bun (graváveis, enumeráveis, configuráveis salvo nota)

alloc(1), allocUnsafe(1), allocUnsafeSlow(1), byteLength(2), compare(2), concat(2), copyBytesFrom(1),
from(3, `name` vazio), isBuffer(1, `name` vazio), isEncoding(1), length (w0 e0 c1), name (w0 e0 c1),
prototype (w0 e0 c0), poolSize = 8192.

## Chaves do protótipo, na ordem

asciiSlice(2) asciiWrite(3) base64Slice(2) base64Write(3) base64urlSlice(2) base64urlWrite(3) compare(5) copy(4)
equals(1) fill(4) hexSlice(2) hexWrite(3) includes(3) indexOf(3) inspect(2) lastIndexOf(3) latin1Slice(2)
latin1Write(3) offset(getter, w0 e0 c1) parent(getter, idem) readBigInt64{,BE,LE}(1) readBigUInt64{,BE,LE}(1)
readDouble{,BE,LE}(1) readFloat{,BE,LE}(1) readInt16{,BE,LE}(1) readInt32{,BE,LE}(1) readInt8(1) readIntBE(2)
readIntLE(2) readUInt16BE/LE readUInt32BE/LE readUInt8 (1) readUIntBE/LE(2) slice(2) subarray(2) swap16/32/64(0)
toJSON(0, `name` vazio) toLocaleString(4, `name` "toString") toString(4) ucs2Slice(2) ucs2Write(3) utf16leSlice(2)
utf16leWrite(3) utf8Slice(2) utf8Write(3) write(4) writeBigInt64BE/LE(3) writeBigUInt64BE/LE(3)
writeDouble{,BE,LE}(2) writeFloat{,BE,LE}(2) writeInt16BE/LE writeInt32BE/LE writeInt8 (2) writeIntBE/LE(3)
writeUInt16 writeUInt16BE/LE writeUInt32 writeUInt32BE/LE writeUInt8 (2) writeUIntBE/LE(3); depois os apelidos
`Uint` (readUintBE/LE, readUint8, readUint16BE/LE, readUint32BE/LE, readBigUint64BE/LE, writeUintBE/LE,
writeUint8, writeUint16{,BE,LE}, writeUint32{,BE,LE}, writeBigUint64BE/LE), cujo `name` é o da forma `UInt`;
por fim `constructor` (w1 e0 c1), `@@toStringTag`, `@@nodejs.util.inspect.custom`, `@@species`.

Obs: `offset` e `parent` lançam `TypeError: Receiver should be a typed array view` em `Buffer.prototype`.
`length` e `name` aparecem no meio das estáticas (entre isEncoding e prototype), e `poolSize` depois de `prototype`.

## Comportamento medido

- `Buffer.from('ab')` hex 6162; `'6162','hex'`; `'YWI=','base64'`; array `[97,98,300]` vira 61622c (módulo 256);
  `Buffer` copia; `ArrayBuffer` COMPARTILHA a memória (`b.buffer === ab` e escrita cruzada, medido na fatia 5;
  a versão anterior deste plano dizia "copiam" e estava errada).
- `new Buffer(-1)` e `new Buffer(2**33)`: `>= 0 and <= 4294967296` (com `and`); NaN/Infinity e `Buffer.alloc` usam `&&`.
- `alloc(3, 'zz', 'hex')` e `alloc(3, Buffer.alloc(0))` lançam `ERR_INVALID_ARG_VALUE`; `alloc(3, '')`, `true`, `{}`, `null`
  e `[5]` não lançam (`010101`, zeros, zeros, `050505`).
- Número grande na mensagem: `addNumericalSeparator` sobre `String(n)` (`1.5e300` vira `1._5e+_300`).
- `'é'` em latin1 = e9; em utf8 = c3a9; `toString('latin1')` do utf8 dá `Ã©`; `toString('ascii')` zera o bit alto
  (`C)`); `[0xe9,0xff].toString('ascii')` = `i\x7f`.
- base64: `ab?>` = `YWI/Pg==`; base64url = `YWI_Pg` (sem padding). Decodificação aceita os dois alfabetos
  (`a-_b` = 6befdb), ignora lixo e para no `=` (`YW\nJj=Zg` = `abc`).
- hex inválido: `'zz'` = 0 bytes, `'abc'` = 1 byte.
- ucs2: `'hé'` = 6800e900; `toString('utf16le')` de 5 bytes dá 2 caracteres.
- `Buffer.from('a', undefined|null)` é utf8; encoding é caso-insensível (`'ASCII'`, `'UTF-8'`).
- `toString('utf8', 1, 2)` = `b`; `toString(undefined, 1)` = `bc`.
- `alloc(3)` zerado, `alloc(3, 1)` = 010101, `allocUnsafe(2).length` 2, `poolSize` 8192.
- `buf.subarray`, `buf.slice` e `buf.map` devolvem `Buffer`; `Buffer.isBuffer(new Uint8Array(1))` falso.
- `Buffer.byteLength('é')` 2, `Buffer.byteLength('YWI=','base64')` 2.
- Erros: encoding ruim (`from` e `toString`) `TypeError` `ERR_UNKNOWN_ENCODING` `Unknown encoding: nope`.
  `Buffer.from(x)` inválido: `TypeError` `ERR_INVALID_ARG_TYPE`, prefixo `The first argument must be of type
  string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. Received ` mais:
  `undefined` | `null` | `type boolean (true)` | `type number (5)` (`-0` mostra `0`) | `type symbol (Symbol(s))` |
  `type bigint (10n)` | `an instance of Object` | `function ` (com espaço e o nome da função).

## Fatias de 5 minutos

1. (feita, não compilada) `node_buffer.rs`: classe, protótipo/ctor herdando de Uint8Array, `from(string|Uint8Array)`,
   `toString(enc,start,end)` com utf8/hex/base64/base64url/latin1/ascii/utf16le.
2. Ligar: chamar `install_buffer` em `js_global_object_init.rs` e conferir a posição de `Buffer` na lista de globais
   (linha ~598); `reset_for_program` no `cell_registry::reset_program_state`; compilar e corrigir.
3. `from` de array, array-like, `ArrayBuffer` (com offset/length), objeto com `type:'Buffer'`; erros dos tipos
   symbol/bigint/função no `invalid_first_argument`.
4. `alloc(size, fill, enc)`, `allocUnsafe`, `allocUnsafeSlow`, `poolSize`, `isBuffer`, `isEncoding`, `byteLength`.
5. `concat`, `compare`, `copyBytesFrom`, `equals`, `compare` do protótipo, `subarray`/`slice` devolvendo `Buffer`
   (estrutura de espécie), `Symbol.species`, `toStringTag`.
6. `inspect` (`<Buffer 61 62>`, limite de 50 bytes, `... N more bytes`), `toJSON`, `toLocaleString`.
7. `xxxSlice`/`xxxWrite`, `write`, `fill`, `indexOf`/`includes`/`lastIndexOf`, `copy`, `swap16/32/64`.
8. Os `read*`/`write*` (inteiros, float, double, BigInt), com os erros `ERR_OUT_OF_RANGE`/`ERR_BUFFER_OUT_OF_BOUNDS`.
9. Ordem das chaves e descritores conferidos por um golden gerado do bun, `offset`/`parent`, `new Buffer`.
10. `process.stdin` `data` sem `setEncoding` entregando `Buffer` (process_stdio.rs).
