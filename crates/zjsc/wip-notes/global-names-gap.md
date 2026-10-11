# Nomes do global: bun 1.4.2 contra o porte

Medição: `bun script.js` com `Object.getOwnPropertyNames(globalThis)` dá 157 nomes. (Com `bun -e` aparecem
mais 29 nomes de módulos do node, `ffi`, `assert`, `fs`... `jsc`: são artefato do modo `-e`, não globais de
um programa e ficam fora desta comparação.) O porte foi lido por código (sem cargo): cada `put_direct` no
global, as tabelas de `js_global_object_init.rs`, `js_global_object_functions_natives.rs`, `timers.rs`,
`queue_microtask.rs` e `js_module_loader.rs`. Não há golden com a lista inteira de nomes do global
(`globals_bun.tsv` e `global_*_bun.tsv` são casos de programa, não inventário).

## Vazamentos (nomes do porte que o bun não tem)

Nenhum. Todo nome instalado no global pelo porte existe no bun: `Infinity`, `undefined`, `NaN`, as funções
globais (`isNaN`...`parseFloat`, `eval`, `globalThis`), os construtores e namespaces JSC, os `Float16Array` e
companhia, `DisposableStack`, `AsyncDisposableStack`, `SuppressedError`, `Iterator`, `ShadowRealm`, `Intl`,
`Temporal`, `WebAssembly`, `console`, e os cinco nomes de host (`queueMicrotask`, `setTimeout`, `setInterval`,
`setImmediate`, `clear*`) mais `ResolveMessage`. Procurados e ausentes do porte (portanto sem vazamento):
`print`, `debug`, `gc`, `readFile`, `load`, `quit`, `$vm`, `$262`, `drainMicrotasks`, `jscStack`, `describe`.

Observação: `WebAssembly` e `Temporal` só nascem sob `Options::use_wasm()` e `Options::use_temporal()`. Com as
opções desligadas o porte fica sem o nome que o bun tem; é desvio de configuração, não vazamento.

## Nomes do bun que o porte não instala

### JSC puro

Nenhum faltando em condição normal (as duas ressalvas acima são de opção). Confirmar com o teste
`builtin_own_keys_golden` quando houver cargo.

### Do bun/WebCore (host)

Já instalados: `global` e `self` (`src/runtime/global_aliases.rs`: `global` é dado comum; `self` é acessor
`get`/`set` que ignora o `this`, o setter redefine `self` no global como dado) e `navigator`
(`src/runtime/navigator.rs`: objeto comum com três acessores `userAgent`, `platform`, `hardwareConcurrency` e
`Symbol.toStringTag` "Navigator"; sem `language`, `languages`, `onLine` nem global `Navigator`; golden
`tests/golden/global_navigator_bun.tsv`, gerador `scripts/gen-global-navigator-golden.js`, runner
`tests/global_navigator_bun_golden.rs`; `hardwareConcurrency` vem da constante `HARDWARE_CONCURRENCY` = 16 e
`userAgent` é `Bun/1.4.2`, únicas fontes do porte, sem leitura da máquina; `self` é instalado depois de
`reorder_standard_globals`, que não move acessores), `queueMicrotask`, `setTimeout`, `setInterval`, `setImmediate`, `clearTimeout`, `clearInterval`,
`clearImmediate`, `ResolveMessage`, `reportError` e `structuredClone` (parcial, ver abaixo).

Faltam (62 nomes; `CustomEvent`, `AbortController` e `AbortSignal` foram portados, ver abaixo), por grupo:

- Mensagens do bundler: `BuildMessage`, `BuildError`, `ResolveError`.
- Eventos e alvo: `CustomEvent` está portado em `event_target.rs` (herda de `Event` via
  `native_class_support::create_native_subclass`; `detail`, `initCustomEvent`, brand check e erros no mesmo golden,
  gerador e runner; reaproveita o estado e o despacho de `Event` sem cópia). A mensagem de `ERR_EVENT_RECURSION`
  agora sai em UTF-16 (`node_error::throw_coded_error_with_message`), com casos de surrogate solto no golden.
  `EventTarget` e `Event` estão portados em `src/runtime/event_target.rs` (golden
  `tests/golden/event_target_bun.tsv`, gerador `scripts/gen-event-target-golden.js`, runner
  `tests/event_target_bun_golden.rs`): construtores, protótipos, constantes de fase, `isTrusted` próprio, brand check,
  erros com `code`, lista de ouvintes (ordem, duplicata, `once`, `handleEvent`, remoção/inclusão durante o despacho,
  `stopImmediatePropagation`, retorno de `dispatchEvent`, `ERR_EVENT_RECURSION`). FALTA: fases de captura e
  propagação (`bubbles`, `composedPath` real), `passive` efetivo, aviso de stderr do ouvinte `null`; o
  golden não cobre exceção de ouvinte (o bun encerra o script). `AbortController` e `AbortSignal` estão portados
  em `src/runtime/abort_signal.rs` (mesmo golden/gerador/runner; `AbortSignal` herda de `EventTarget` via
  `create_native_subclass`): forma e descritores, `abort(reason)` com o `DOMException` `AbortError` padrão,
  `aborted`, `reason`, `throwIfAborted`, `onabort` (atributo de evento que ocupa a posição do primeiro `set`),
  evento `abort` confiável (`isTrusted` `true`), `AbortSignal.abort()`, `AbortSignal.any()` (dependentes
  encadeados) e a opção `signal` de `addEventListener` (valida, não registra sinal abortado, remove no aborto).
  FALTA `AbortSignal.timeout(ms)`: o `timers.rs` só agenda callback JS, falta um timer nativo; a chave
  `timeout` some do `Object.keys(AbortSignal)`. Ainda ausentes: `addEventListener`,
  `removeEventListener`, `dispatchEvent` globais (o global do bun é um `EventTarget`), `ErrorEvent`,
  `CloseEvent`, `MessageEvent`. (`DOMException`
  foi portado em `src/runtime/js_dom_exception.rs`, golden `tests/golden/dom_exception_bun.tsv`; entra no fim da
  ordem de chaves, junto de `ResolveMessage`.)
- Diálogos e relatório: `onmessage`, `onerror`. (`alert`, `confirm` e `prompt` foram portados em
  `src/runtime/dialogs.rs`, golden `tests/golden/dialogs_bun.tsv` com 141 casos, gerador
  `scripts/gen-dialogs-golden.js`, runner `tests/dialogs_bun_golden.rs`: com stdin em EOF devolvem `undefined`,
  `false` e `null`; convertem o primeiro argumento com `ToString` (o `prompt` também o segundo, mesmo com o
  primeiro `undefined`); `length` 1, não construtores. DIVERGÊNCIA: o convite que o bun escreve no stdout
  (`Alert q [Enter] `...) não sai, e a leitura real de stdin interativo não existe. Escrito sem cargo, não
  compilado nem rodado.) (`postMessage` foi portado em
  `src/runtime/post_message.rs`, golden `tests/golden/post_message_bun.tsv` com 78 casos, gerador
  `scripts/gen-post-message-golden.js`, runner `tests/post_message_bun_golden.rs`: na thread principal é um no-op que
  devolve `undefined`, `length` 1, sem tocar nos argumentos nem clonar, e `new` é `TypeError`. `onmessage` NÃO foi
  portado: o bun mantém o processo vivo quando um handler é atribuído, e isso é laço de eventos de worker. Novo
  helper `install_global_function` em `native_class_support.rs`, já usado por `reportError` e `queueMicrotask`.
  Escrito sem cargo, não compilado nem rodado.)
- Base64 e clones: `fetch`. (`atob` e `btoa` foram portados em `src/runtime/base64_globals.rs`,
  golden `tests/golden/base64_globals_bun.tsv`; o erro de caractere inválido é um `DOMException`
  `InvalidCharacterError` real.)
- Objetos de processo: `Bun`, `process`.
- `performance` e as classes `Performance`, `PerformanceEntry`, `PerformanceMark`, `PerformanceMeasure`,
  `PerformanceTiming` estão portados em `src/runtime/performance.rs` (golden `tests/golden/performance_bun.tsv`, gerador
  `scripts/gen-performance-golden.js`, runner `tests/performance_bun_golden.rs`): descritores, protótipos,
  construtores ilegais, `now()` (monotônico, fração) e `timeOrigin` do relógio de `Date.now`
  (`wtf::date_math::current_time_in_nanoseconds`), `mark`, `measure` (posicional e com objeto), `getEntries*`,
  `clearMarks`, `clearMeasures`, `toJSON` (com o `timing`), `timing`, `onresourcetimingbufferfull`,
  `clearResourceTimings`, `setResourceTimingBufferSize`, `markResourceTiming` e os erros exatos. `Performance`
  herda de `EventTarget` (construtor e protótipo); `performance` é um alvo de eventos registrado. O
  `onresourcetimingbufferfull` só guarda o valor, não dispara nada.
  `PerformanceObserver`, `PerformanceObserverEntryList`, `PerformanceResourceTiming` e
  `PerformanceServerTiming` estão portados em `src/runtime/performance_observer.rs` (mesmo golden, 841 casos): forma
  dos construtores e protótipos, erros de construção, `observe` (validação, ordem de leitura, `InvalidModificationError`),
  `disconnect`, `takeRecords` alimentado por `mark`/`measure`, `supportedEntryTypes`, e getters/métodos das três
  classes sem instância sempre com o erro de `this` inválido. PENDENTE: disparo do callback do observador (precisa do
  agendador e do `PerformanceObserverEntryList` entregue a ele), `buffered: true`, `entryTypes` iterável que não é
  array, `supportedEntryTypes` devolver array novo a cada leitura (no bun `===` é `false`), o buffer de recursos
  (`getEntriesByType('resource')`, sem `fetch`) e as instâncias de `PerformanceResourceTiming`/`PerformanceServerTiming`.
  Posição no global: a tabela `ORDER` de `js_global_object_init.rs` já tem `PerformanceTiming` depois de
  `PerformanceServerTiming` (índices 59 a 67 no bun, medidos), e `reorder_standard_globals` o reposiciona; o golden
  confere a diferença contra `MessagePort`. Escrito sem cargo, não compilado nem rodado.
  Os valores de tempo entram no golden só por tipo e relação.
- Arquivos e rede: `File`, `Blob`, `Buffer`, `Request`, `Response`, `Headers`, `FormData`, `WebSocket`,
  `Worker`, `HTMLRewriter`, `URL`, `URLPattern`, `URLSearchParams`, `BroadcastChannel`, `MessageChannel`,
  `MessagePort`.
- Cripto e texto: `crypto`, `Crypto`, `CryptoKey`, `SubtleCrypto`, `TextDecoder`, `TextEncoder`,
  `TextDecoderStream`, `TextEncoderStream`.
- Streams: `ReadableStream`, `ReadableStreamBYOBReader`, `ReadableStreamBYOBRequest`,
  `ReadableStreamDefaultController`, `ReadableStreamDefaultReader`, `ReadableByteStreamController`,
  `WritableStream`, `WritableStreamDefaultController`, `WritableStreamDefaultWriter`, `TransformStream`,
  `TransformStreamDefaultController`, `CompressionStream`, `DecompressionStream`. (`ByteLengthQueuingStrategy`
  e `CountQueuingStrategy` foram portados, ver a seção abaixo.)

Fora do alcance do porte do JavaScriptCore (são WebCore e runtime do bun, não JSC). Quem for ao sandbox
completo implementa por cima; `BuildMessage`/`BuildError`/`ResolveError` são o par natural do
`ResolveMessage` que já existe, mas dependem do bundler do bun.

## `reportError` e `structuredClone` (portados sem cargo, não compilados nem rodados)

Medido no bun 1.4.2 (`scripts/gen-structured-clone-golden.js`, goldens `tests/golden/structured_clone_bun.tsv` com
154 casos (o antigo golden pendente foi removido: tudo que o bun clona o porte clona); runner `tests/structured_clone_bun_golden.rs`):

- Ambos: propriedade de dados `writable`/`enumerable`/`configurable`, não construtor (`new` lança TypeError),
  `toString()` "function NOME() { [native code] }", só `length` e `name` próprios. Ordem de chaves:
  `queueMicrotask`, `reportError`, `setImmediate`, `setInterval`, `setTimeout`, `structuredClone`, `global`.
  `reportError.length` 1, `structuredClone.length` 2.
- `reportError(x)`: qualquer valor (ou nenhum), devolve `undefined`, não lança, o código seguinte roda; o erro vai ao
  `uncaughtException` (sem handler, o bun imprime e sai com 1 no fim). Porte: `src/runtime/report_error.rs`, usa
  `vm.report_unhandled_error`. Falta o `ErrorEvent` em `addEventListener("error")`/`onerror` (sem `EventTarget`).
- `structuredClone`: sem argumento `TypeError: structuredClone requires 1 argument`; opções que não são objeto/null/
  undefined dão `TypeError: Type error`; `transfer: 5` dá `TypeError` com `code` `ERR_INVALID_ARG_TYPE` ("Optional
  options.transfer argument must be an iterable"). Ciclos e referências repetidas preservam identidade; classe perde o
  protótipo (vira `Object`); getters rodam e viram propriedade de dados; não enumeráveis e chaves Symbol somem;
  buraco de array continua buraco; função e Symbol (também aninhados) lançam `DataCloneError`, "The object can not be
  cloned.", `code` 25, que é um `DOMException`. Erro de getter propaga sem envolver.
- Portado: primitivos, `Object` comum, `Array`, ciclos, os erros acima (`src/runtime/structured_clone.rs`; o
  `DataCloneError` é um `DOMException` real, `throw_dom_exception` de `js_dom_exception.rs`).
- Pendente, só no golden `pending` (o porte lança `DataCloneError` onde o bun clona): Date (preserva o tempo, perde
  propriedades extras), RegExp (`source`, `flags`, `lastIndex` volta a 0), Map, Set (com ciclos), ArrayBuffer,
  redimensionável (mantém `maxByteLength`), SharedArrayBuffer (devolve outro objeto, ainda `SharedArrayBuffer`),
  typed arrays e `DataView` (buffer compartilhado entre views preservado), Error e subclasses (volta o construtor
  nativo pelo `name` quando é de erro nativo, senão `Error`; fica só `message`, `stack` e as posições, `cause` e
  propriedades extras se perdem), invólucros `Number`/`String`/`Boolean`/`BigInt`, e a opção `transfer` (destaca o
  buffer: `byteLength` 0, `detached`; lista com buffer repetido é `DataCloneError` "Transfer list contains duplicate
  ArrayBuffer"; item que não é transferível é `DataCloneError`; hoje `transfer` com valor é `Thrown::Unported`).
  Também sem limite de profundidade de recursão como o do bun.

## `TextEncoder` (portado sem cargo, não compilado nem rodado)

Medido no bun 1.4.2 (`scripts/gen-text-encoder-golden.js`, golden `tests/golden/text_encoder_bun.tsv` com 215 casos,
runner `tests/text_encoder_bun_golden.rs`, porte `src/runtime/text_encoder.rs`, instalado em
`js_global_object_init.rs` depois do `DOMException`):

- Propriedade de dados `writable`/`enumerable`/`configurable`; construtor `length` 0, chaves próprias `length`,
  `name`, `prototype`; protótipo herda de `Object.prototype` com `constructor`, `encoding` (getter `get encoding`,
  enumerável), `encode` (length 1), `encodeInto` (length 2) e `@@toStringTag`. Instância sem propriedade própria;
  `new TextEncoder(qualquer coisa)` ignora os argumentos.
- Sem `new`: `TypeError` "Use `new TextEncoder(...)` instead of `TextEncoder(...)`", `code` `ERR_ILLEGAL_CONSTRUCTOR`.
  `this` inválido: `encode`/`encodeInto` dão "Can only call TextEncoder.encode on instances of TextEncoder"
  (`ERR_INVALID_THIS`); o getter dá "The TextEncoder.encoding getter can only be used on instances of TextEncoder"
  sem `code`.
- `encode`: sem argumento ou `undefined` é `""`; unidade substituta solta vira U+FFFD; devolve `Uint8Array` com buffer
  exato (`encode('')` tem `byteLength` 0).
- `encodeInto`: menos de 2 argumentos é `TypeError: Not enough arguments` (`ERR_MISSING_ARGS`, `code` própria); a fonte
  é convertida antes de olhar o destino; qualquer `ArrayBufferView` serve (`Uint16Array`, `DataView`, `Buffer`,
  `Uint8ClampedArray`, preenchidos como bytes), o resto é `TypeError: Expected Uint8Array`; só entram pontos de
  código inteiros (`'€'` em 2 bytes dá `{read:0,written:0}`); devolve `{read, written}` com `read` em unidades UTF-16.
- Divergências anotadas no cabeçalho do porte: `code` de `ERR_ILLEGAL_CONSTRUCTOR`/`ERR_INVALID_THIS` é própria aqui
  e herdada no bun; o erro nativo do bun traz `originalLine`, `line`, `column`, `sourceURL`; a chave entra no fim
  da ordem do global (no bun fica entre `TextDecoderStream` e `TextEncoderStream`, índice 76; `TextDecoder` é 39).
- Brand check por conjunto de valores codificados em `thread_local` (zerado em `reset_for_program`, linha nova em
  `cell_registry.rs`), como `ResolveMessage`, sem variante nova de `CellEntry`.

### `TextDecoder` (portado em `src/runtime/text_decoder.rs`, golden `text_decoder_bun`; só as codificações utf-8, utf-16le/be e windows-1252)

- Global `writable/enumerable/configurable`, `length` 0. Protótipo, nesta ordem: `decode` (length 1, enumerável),
  `encoding`, `fatal`, `ignoreBOM` (getters `get encoding`..., enumeráveis), `constructor`, `@@toStringTag`
  (note que `decode` vem antes de `constructor`, diferente do `TextEncoder`). Instância sem chaves próprias.
- Sem `new`: `TypeError` "TextDecoder constructor cannot be invoked without 'new'" (`ERR_ILLEGAL_CONSTRUCTOR`).
  Rótulo desconhecido: `RangeError` `Unsupported encoding label "x"` (`ERR_ENCODING_NOT_SUPPORTED`). Rótulos
  normalizam (`UTF8` vira `utf-8`, `latin1` vira `windows-1252`, `utf-16le`). `fatal` e `ignoreBOM` coagem para booleano.
- `decode()` sem argumento é `""`; BOM inicial é removido (`ignoreBOM` false); byte inválido vira U+FFFD, e com
  `fatal: true` é `TypeError` "The encoded data was not valid for encoding utf-8" (`ERR_ENCODING_INVALID_ENCODED_DATA`);
  `decode(5)` é `TypeError` "TextDecoder.decode expects an ArrayBuffer or TypedArray" (`ERR_INVALID_ARG_TYPE`);
  `{stream: true}` guarda sequência incompleta; `this` inválido: "Expected this to be instanceof TextDecoder, but
  received an instance of Object" (`ERR_INVALID_THIS`, formato do `describe_received` de `js_module_loader.rs`).

## `CountQueuingStrategy` e `ByteLengthQueuingStrategy` (portados sem cargo, não compilados nem rodados)

Medido no bun 1.4.2 (`scripts/gen-queuing-strategy-golden.js`, golden `tests/golden/queuing_strategy_bun.tsv` com
164 casos, runner `tests/queuing_strategy_bun_golden.rs`, porte `src/runtime/queuing_strategy.rs`, um só módulo com
um `Kind` e um `macro_rules!` que estampa as cinco funções nativas de cada classe; instalado em
`js_global_object_init.rs` depois do `TextDecoder`, na posição que o `ORDER` já tinha para os dois nomes):

- Propriedade global `writable`/`configurable` e NÃO enumerável (diferente de `TextEncoder`). Construtor `length` 1,
  chaves próprias `length`, `name`, `prototype`. Protótipo: `constructor`, acessores `highWaterMark` e `size`
  (enumeráveis, só getter), `Symbol(nodejs.util.inspect.custom)`, `@@toStringTag`.
- `new X(init)`: sem argumento `Not enough arguments`; `undefined`, `null` ou sem o membro `QueuingStrategyInit
  requires a 'highWaterMark' member`; não objeto `The QueuingStrategyInit argument must be an object`; o membro
  passa por `ToNumber` (`'7'` vale 7, `NaN`, `-1`, `Infinity` valem); `Symbol` e `BigInt` lançam `TypeError`.
  Sem `new`: `ERR_ILLEGAL_CONSTRUCTOR`; `this` alheio nos getters: `Value of "this" must be of type X`
  (`ERR_INVALID_THIS`). `size` é uma só função por classe (`length` 0 na `Count`, 1 na `ByteLength`), a `Count`
  devolve 1 e a `ByteLength` devolve `chunk.byteLength` sem conversão.
- Mudança de apoio: `create_native_class_with_length` e `install_global_with_attributes` em
  `native_class_support.rs` (as versões antigas viraram chamadas delas, sem mudar os demais chamadores).
- Divergências: falta o `Symbol(nodejs.util.inspect.custom)` (depende do `inspect` do `util`, que o porte não tem);
  o `TypeError` de `size(chunk)` com `chunk` não objeto sai `X is not an object` e o bun embute o texto da chamada.
  Os dois ficam fora do golden.

## Ação

Sem vazamento a corrigir. O código novo desta rodada (`report_error.rs`, `structured_clone.rs`, a extração
`throw_dom_exception_stand_in` em `base64_globals.rs`, o registro em `js_global_object_functions_natives.rs`,
`text_encoder.rs`) nunca foi compilado: o próximo passo com cargo é `cargo test --test structured_clone_bun_golden
--test text_encoder_bun_golden`.
