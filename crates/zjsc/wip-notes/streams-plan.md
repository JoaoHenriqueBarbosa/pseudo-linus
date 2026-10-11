# Plano: streams web (ReadableStream, WritableStream, TransformStream) para passar `tests/golden/streams_bun.tsv`

Investigação de 2026-10-09, sem implementação.

## Achado central

No bun 1.4.2 os streams NÃO são builtins JS. `src/js/builtins/` tem só 17 arquivos (Bake, Fifo, Glob, Ipc, ConsoleObject,
shell, WasmStreaming...), nenhum `*Stream*.ts` nem `*Internals.ts`. Os streams são C++ nativo em
`src/jsc/bindings/webcore/streams/`: 80 arquivos, 23215 linhas. Logo não há `.ts` para reaproveitar
nem transpilação de TS a decidir: o caminho é portar C++ para Rust, como já se faz com o resto do `runtime`.
Os intrínsecos privados (`@putByIdDirectPrivate`, `@createFIFO`, `$isReadableStream`) não são exigidos: o estado
vive em campos de células C++ (`WriteBarrier`, `StreamQueue`), não em propriedades privadas.

## Mecanismo de builtins JS do porte (para referência)

`scripts/gen-builtins.py` lê `derived/JavaScriptCore/JSCBuiltins.{cpp,h}` (saída do gerador do próprio JSC sobre
`builtins/*.js` do JSC, os 102 builtins) e emite `src/runtime/builtins_source.rs` e `builtins_combined.js`
(`include_str!`). Só cobre os `.js` do JSC; os `src/js/**/*.ts` do bun (node:stream etc.) usam o pré-processador
próprio do bun (`$` e `@` para nomes privados, `require` embutido) e não estão ligados. Não serve a streams.

## Composição (linhas, C++ do bun)

Maiores: BunStreamConsumers.cpp 1727, ReadableStreamOperations.cpp 1575, BunStreamSource.cpp 1447,
JSReadableByteStreamController.cpp 1219, JSDirectStreamController.cpp 1041, JSReadableStreamDefaultReader.cpp 789,
JSReadableStream.cpp 786, WebStreamsInternals.h 733, JSStreamPipeToOperation.cpp 652, JSWritableStreamDefaultController.cpp 622,
JSReadableStreamDefaultController.cpp 619, WebStreamsMisc.cpp 613, WritableStreamOperations.cpp 584,
BunAsyncIterableSource.cpp 557. Resto: classes pequenas (Writer, TransformStream, TransformStreamOperations,
BYOBReader/Request, TeeState, PullIntoDescriptor, StreamQueue.h, strategies, Compression/Decompression, TextEncoder/DecoderStream,
CrossRealmTransform, WebStreamsInspectCustom, WebStreamsExports).

Fora do alvo da golden (adiar): BunStreamConsumers, BunStreamSource, JSDirectStreamController, BunAsyncIterableSource,
CrossRealmTransform, Compression/Decompression, JSReadStreamIntoSinkOperation, JSOneShotDirectSink (~6500 linhas, dependem de
Blob/Response/zlib/Bun.*). Núcleo da spec WHATWG: ~12000 linhas.

## O que a golden exige (314 casos)

Maior parte é forma: `Reflect.ownKeys` de construtores e protótipos, descritores (`DESC`), `length`/`name`, toStringTag,
chaves como `readMany`, `blob`/`bytes`/`json`/`text`, `Symbol(nodejs.util.inspect.custom)`. Depois comportamento:
114 `new ReadableStream`, 43 `new WritableStream`, 27 `new TransformStream`, mais Reader, BYOBReader, Writer, controllers,
TextDecoderStream/TextEncoderStream (poucos), Compression (4, adiar).

## O que o porte já tem

Presente: `JSPromise`, Promise/microtasks, `text_decoder.rs`, `blob.rs`, `web_iterable.rs`, `host_function_support.rs`,
células sem GC (`cell_registry.rs`), `JSArrayBufferView`/ArrayBuffer/typed arrays, geradores de golden. Ausente (PLAN.md linha 825):
`ReadableStream` e família, `TransformStream`, `WritableStream`, `Headers`/`Request`/`Response` (consumidores de stream), `URL`.
Falta verificar na hora de portar: como `blob.rs` define classe host com protótipo e getters (molde para as classes de stream),
e como o porte representa `WriteBarrier`/células nativas com campos JSValue.

## Estratégia

Portar o C++ fielmente como módulo Rust `runtime/streams/` (um `.rs` por `.cpp`/`.h`, mesma divisão), classes como as de
`blob.rs`, sem reescrever em JS (JS exporia frames/`toString` diferentes, e o bun é nativo: `ReadableStream.toString()` deve
dar `function ReadableStream() { [native code] }`).

## Fatias de ~5 min, em ordem

1. Ler `blob.rs` e `web_iterable.rs`, escrever o molde de classe host (construtor, protótipo, toStringTag, brand check) e registrar os globais vazios `ReadableStream`, `WritableStream`, `TransformStream`. Alvo: casos de `DESC(globalThis, ...)`.
2. `StreamQueue.h`, `StreamsForward.h`, `WebStreamsInternals.h` (tipos e helpers).
3. `ByteLengthQueuingStrategy`, `CountQueuingStrategy` (pequenas, isoladas).
4. `JSReadableStream` shell: construtor, getters `locked`, `cancel`, `getReader`, `tee`, ownKeys da golden.
5. `ReadableStreamDefaultController` + `ReadableStreamOperations` parte 1 (SetUp, Enqueue, Close, Error, CallPullIfNeeded).
6. `JSReadableStreamReaderBase` + `JSReadableStreamDefaultReader` (`read`, `readMany`, `releaseLock`, `closed`) + `JSReadRequest`.
7. `ReadableStreamOperations` parte 2 (cancel, tee, `JSStreamTeeState`) e `pipeThrough`/`pipeTo` stubs fiéis.
8. `JSWritableStream` + `WritableStreamOperations` + `WritableStreamDefaultController` + `Writer` (4 a 5 fatias).
9. `JSTransformStream` + `TransformStreamOperations` + `TransformStreamDefaultController`.
10. `JSStreamPipeToOperation` (652 linhas, 2 fatias) e `ReadableStream.from`, iterador assíncrono (`values`, `JSReadableStreamAsyncIterator`).
11. `JSReadableByteStreamController`, `BYOBReader`, `BYOBRequest`, `PullIntoDescriptor` (1219 + ~700, 3 fatias).
12. `TextEncoderStream`/`TextDecoderStream` (usa `text_decoder.rs`).
13. `ReadableStream.prototype.blob/bytes/json/text` (BunStreamConsumers parcial, depende de Blob).
14. Rodar `streams_bun.tsv` por grupo, corrigir divergências; CompressionStream/DecompressionStream por último (precisa zlib/`zdeflate`).

Estimativa: ~30 fatias de 5 min. Primeiro marco útil (fatias 1 a 6) cobre a maioria dos casos de forma e leitura básica.
