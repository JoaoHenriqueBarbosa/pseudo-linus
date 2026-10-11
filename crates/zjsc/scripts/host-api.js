// Filtro de APIs de host para os geradores de golden.
//
// O porte é só o motor JSC: um golden cujo resultado depende de API que o host do bun
// fornece (structuredClone, queueMicrotask, setTimeout, Bun, process, Buffer, fetch,
// TextEncoder, URL, atob/btoa, console, require/module, DOMException, streams e o resto
// das Web APIs, import.meta.url e afins) não mede o motor. Isso inclui `typeof X`, que
// no bun dá 'function' e no porte dá 'undefined', e a referência solta a X.
//
// `typeof window` e `typeof document` não entram: dão 'undefined' no bun e em qualquer
// motor puro, então o resultado é do motor.
"use strict";

const HOST_NAMES = [
  "structuredClone", "queueMicrotask", "setTimeout", "atob", "btoa", "process", "Bun", "Buffer",
  "fetch", "TextEncoder", "TextDecoder", "URL", "URLSearchParams", "DOMException", "ReadableStream",
  "console", "require", "__dirname", "__filename",
  "BroadcastChannel", "CompressionStream", "Crypto", "CustomEvent", "ErrorEvent", "FormData",
  "MessageEvent", "PerformanceEntry", "PerformanceObserver", "PerformanceServerTiming",
  "ReadableStreamDefaultController", "ResolveError", "TextDecoderStream", "TransformStream",
  "URLPattern", "Worker", "WritableStreamDefaultWriter",
  "AbortSignal", "AbortController", "Event", "EventTarget", "Blob", "Headers", "Request", "Response", "File",
  "WebSocket", "MessageChannel", "MessagePort", "WritableStream", "WritableStreamDefaultController",
  "ReadableStreamDefaultReader", "ByteLengthQueuingStrategy", "CountQueuingStrategy", "DecompressionStream",
  "crypto", "CryptoKey", "SubtleCrypto", "performance", "Performance", "PerformanceMark", "PerformanceMeasure",
  "navigator", "Navigator", "setInterval", "setImmediate", "clearTimeout", "clearInterval", "clearImmediate",
  "reportError",
];

const bare = new RegExp("(?<![A-Za-z0-9_$.#])(?:" + HOST_NAMES.slice().sort((a, b) => b.length - a.length).join("|") + ")(?![A-Za-z0-9_$])");
const hostTypeof = /typeof\s+(?:global|module|exports)(?![A-Za-z0-9_$])|(?<![A-Za-z0-9_$.])module\.exports(?![A-Za-z0-9_$])/;
const importMeta = /import\.meta\.(?:url|dir|file|path|dirname|filename|resolve|main|env|require|hot)(?![A-Za-z0-9_$])|(?:keys|entries|values|getOwnPropertyNames|stringify|assign)\(import\.meta\)|in import\.meta/;

// `typeof self` só é do host quando `self` não é um nome local do programa.
const typeofSelf = /typeof\s+self(?![A-Za-z0-9_$])/;
const declaresSelf = /(?:var|let|const|function|class|as)\s+self(?![A-Za-z0-9_$])|[(,]\s*self\s*[,)=]/;

function usesHostApi(program) {
  const text = typeof program === "string" ? program : JSON.stringify(program);
  if (bare.test(text) || hostTypeof.test(text)) return true;
  if (typeofSelf.test(text) && !declaresSelf.test(text)) return true;
  // import.meta fora de módulo é SyntaxError do motor; só os fixtures .mjs leem as propriedades do host.
  return text.includes(".mjs") && importMeta.test(text);
}

module.exports = { usesHostApi, HOST_NAMES };
