// O começo de `node_stream_modules.js` (gerado por `gen-node-stream.js`): a fábrica do módulo, com o que os intrínsecos
// do JSC (`$debug`, `$toClass`, `$ERR_*`, ...) viram aqui. `host.internal` é o `node_internal.js`.
(function (host) {
  "use strict";

  const {
    codes,
    makeAbortError,
    validators,
    aggregateTwoErrors,
    SafeSet,
    SafeArrayIterator,
    kEmptyObject,
    resistStopPropagation,
    once,
    addAbortListener,
    toClass,
  } = host.internal;
  const kAutoDestroyed = Symbol("kAutoDestroyed");
  const kWeakHandler = Symbol("kWeak");

  const debug = () => {};
  const assert = () => {};
  const isCallable = value => typeof value === "function";

  let BufferConstructor;
  const getBuffer = () => (BufferConstructor ??= host.requireBuiltin("node:buffer").Buffer);

  // `$inheritsReadableStream` e companhia: o teste de classe dos web streams.
  const inheritsReadableStream = value => typeof ReadableStream === "function" && value instanceof ReadableStream;
  const inheritsWritableStream = value => typeof WritableStream === "function" && value instanceof WritableStream;
  const inheritsTransformStream = value => typeof TransformStream === "function" && value instanceof TransformStream;
  const inheritsBlob = value => typeof Blob === "function" && value instanceof Blob;

  // DIVERGÊNCIA: os dois ganchos de web stream do bun (`$webStreamControllerError`, `$webStreamClosedPromise`) falam com
  // o interior do `ReadableStream` e ainda não têm equivalente aqui.
  const webStreamControllerError = () => {
    throw new Error("node:stream com web streams ainda não é suportado");
  };
  const webStreamClosedPromise = () => {
    throw new Error("node:stream com web streams ainda não é suportado");
  };

