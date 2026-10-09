# process.stdout / stderr / stdin: plano medido (bun 1.4.2, saída em pipe)

## Medido

stdout e stderr (idênticos): `WriteStream`, chaves próprias na ordem `fd`, `_writev`, `flush`, `start`, `pos`,
`bytesWritten`, `_write` (`underscoreWriteFast`, 3), `write` (`writeFast`, 3), `_construct`, `_events`,
`_writableState`, `_maxListeners`, `readable`, `_type`, `destroySoon` (nome vazio, 2), `_destroy` (vazio, 2), `_final`
(vazio, 1), `_isStdio`. Todas enumeráveis, graváveis e configuráveis. `_writableState`: `highWaterMark` 65536,
`length`, `corked`, `onwrite` (`bound onwrite`, 1), `writelen`, `bufferedIndex`, `pendingcb`.

Cadeia: `WriteStream.prototype` (constructor 2, open, _construct, _write, _writev, _destroy, close, destroySoon,
autoClose, pending), `Writable.prototype` (constructor 1, pipe, write, cork, uncork, setDefaultEncoding, _write,
_writev (null), end, closed, destroyed, writable, writableFinished, writableObjectMode, writableBuffer,
writableEnded, writableNeedDrain, writableHighWaterMark, writableCorked, writableLength, errored, writableAborted,
destroy, _undestroy, _destroy), `Stream.prototype` (constructor 1, pipe, eventNames), `EventEmitter.prototype`,
`Object.prototype`. Os acessores do Writable são `get`, não enumeráveis, não configuráveis.

Comportamento: `cork` duas vezes e `uncork` uma dá `writableCorked` 2 e depois 1; `setDefaultEncoding('bogus')`
lança `TypeError ERR_UNKNOWN_ENCODING: Unknown encoding: bogus`, e devolve `this` quando válido; `end('x\n', cb)`
escreve, devolve `this`, `writableEnded` true, `writable` false, `writableFinished` e `destroyed` false de imediato;
`write` depois de `end` vira erro assíncrono não capturado `ERR_STREAM_WRITE_AFTER_END` (`write after end`).

stdin: `ReadStream` sobre `Readable` (constructor, destroy, push, unshift, isPaused, setEncoding, read, pipe, on,
resume, pause, iterator, ... e os helpers `map`, `filter`, `toArray`...), chaves próprias `fd`, `start`, `end`, `pos`,
`bytesRead`, `_events`, `_readableState`, `_maxListeners`, `_eventsCount`, `on`, `addListener`, `ref`, `unref`,
`pause`, `resume`, `read`, `_read`. Com EOF: `end` e depois `close`, sem `data`. Com `ab\n` e `setEncoding('utf8')`:
`data "ab\n"` e `end`.

## Feito nesta fatia (src/runtime/process_stdio.rs)

Chaves próprias do stdout/stderr na ordem, protótipos compartilhados com os nomes e comprimentos medidos, `cork`,
`uncork`, `setDefaultEncoding`, `end` (escreve e marca), acessores do Writable, `on`/`once`/`off`/`emit`/`listeners`/
`listenerCount`/`eventNames` sobre `ListenerTable` (event_emitter_core.rs) por descritor. stdin: só `fd` e a cadeia de
protótipos (`ReadStream`, `Readable`, `Stream`, `EventEmitter`).

## Medido e feito na fatia do stdin (bun 1.4.2)

`_eventsCount` inicial do stdin é 5 (os quatro internos `close`, `end`, `resume`, `pause` mais a chave símbolo
`kConstruct`, que `Object.keys(_events)` não lista; `eventNames()` a mostra). Cada interno: `name` vazio, `length` 0,
`toString` `function () { [native code] }`, `listeners(nome).length` 1, o MESMO objeto de `_events`. `on('data')` leva a
6, `on('readable')` a 7; `pause()`/`resume()` NÃO mexem em `_eventsCount` (o "7" é o de `data` mais `readable`);
`isPaused()` vira `true` após `pause()`. Passados ~50 ms a contagem cai um (6): o `kConstruct` é consumido (não portado).
Com EOF e com pipe: `end` e depois `close`; com `ab\n` e `setEncoding('utf8')`, `data "ab\n"` antes. `new Stream()` e
`Writable()` têm tabela própria: `st.on('x')` dá `_eventsCount` 1, `Object.keys(st._events)` `["x"]`, e o stdout não vê.
Feito: ouvintes internos reais na tabela do stdin (mesmo objeto), contagem 5 e +1 por evento, `Readable.prototype.setEncoding`,
leitura do stdin pelo `ConsoleHost::read_stdin_line` e emissão de `data`/`end`/`close` num tick, 4 casos novos no golden.

## Fatia seguinte (bun 1.4.2, medido)

Feito: `Slot` por identidade (`encode` do valor) para `new Stream()`/`Writable()`/`Readable()`/`WriteStream(path)`/
`ReadStream(path)`; `ConsoleHost::read_stdin_rest` (bytes, inclusive a última linha sem `\n`); `isPaused()` e
`readableFlowing` reais (início `false`/`null`; `data` `true`/`false`; `pause()` `false`/`true`; `resume()` volta;
`readable` trava em `false`/`true` e `pause`/`resume` não mudam mais; `pause()` e depois `data` fica `false`/`true`);
`kConstruct` consumido num microtask depois dos `nextTick` (sem ouvinte: tick 5, microtask 4; com `data`: 6 e depois 5).
Cinco casos novos no gerador do process (golden não regenerado, cargo proibido; nada compilado nem rodado nesta fatia).

Falta do stdin (resolvido na fatia seguinte, ver abaixo): `data` sem `setEncoding`, efeito dos quatro internos,
`readableFlowing` com nome `get` e setter, `isPaused` sobre estado próprio.

## Fatia do Buffer (bun 1.4.2, medido, NADA compilado: cargo proibido)

- `data` sem `setEncoding` entrega `Buffer` (`constructor.name` `Buffer`, `Buffer.isBuffer` true). Pipe com 200000 bytes:
  pedaços 65536, 65536, 65536, 3392. Arquivo (`< arq`): um pedaço de 200000. `/dev/null`: nenhum `data`. 3 bytes: um de 3.
  O sandbox usa pedaços de 65536 (`STDIN_PIPE_CHUNK`). Com `setEncoding` o porte ainda emite texto único (lacuna).
- Depende de `node_buffer::buffer_from_bytes` ser `pub(crate)` (hoje `fn` privada em node_buffer.rs): o process_stdio
  já a chama assim, falta só a visibilidade.
- `readableFlowing`: acessor em `Readable.prototype`, não enumerável nem configurável, getter `name` `get` (length 0),
  setter `name` `set` (length 1). O getter devolve `this._readableState.flowing` (num objeto qualquer lê a propriedade);
  sem `_readableState` lança `TypeError: undefined is not an object (evaluating 'this._readableState')`. O setter
  grava `_readableState.flowing` cru (sem chave própria no stream, `isPaused()` não muda) e é no-op sem estado.
- `isPaused` lê os bits de `_readableState`: `isPaused.call({_readableState:{flowing:false}})` dá `false`, também com
  `{paused:true}`, `{flowing:true}`, `{flowing:null}` e `{}`; sem `_readableState` lança o mesmo `TypeError`.
  No stdin real: `on('data')` + `pause()` dá `flowing false`, `isPaused true`; depois `readableFlowing = true` dá
  `flowing true` e `isPaused` continua `true` (o bit `kPaused` fica).
- Os quatro internos (`close`, `end`, `resume`, `pause`) chamados à mão: devolvem `undefined`, `flowing` fica `null`
  (ou o que já era), `isPaused`, `destroyed` (`false`), `readable` (`true`) e `_readableState.closed` (`false`) não mudam.
  Portanto são no-ops de verdade, o que o porte já fazia.
- Cinco casos novos no gerador do process (golden não regenerado).


## Falta

1. Eventos `finish`/`close` e callback de `end` (assíncronos), erro de `write` após `end`.
2. stdin completo: chaves próprias, `_readableState`, leitura do pipe, `on('data'/'end'/'close')`, `setEncoding`, `read`,
   `pause`/`resume`, métodos do `Readable.prototype` e os helpers de iterador.
3. `prependListener`, `removeAllListeners`, `setMaxListeners`, `getMaxListeners`, `_eventsCount`, `newListener`.
4. `writableBuffer` (o getter devolve `undefined`, não medido), `pipe`, `destroy` real, `open`/`close`/`_construct`.
5. Gerar golden TSV para 644 a 647 e as chaves/descritores acima; ainda não rodado (cargo proibido nesta fatia).
