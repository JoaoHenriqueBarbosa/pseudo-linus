// Gera tests/golden/options_parity_bun.tsv: presença (typeof) de globais e métodos que dependem das `Options` do
// JavaScriptCore, medida no bun 1.4.2. Colunas: a expressão e o resultado de `typeof (expressão)`, ou
// `throws <Nome>` quando avaliar a expressão lança (o `Temporal.Now` sem `Temporal`, por exemplo).
// As ausências importam tanto quanto as presenças: o sandbox responde `undefined`/`ReferenceError` como o bun.
// O programa de medição é o mesmo que `tests/options_bun_parity.rs` monta (`probe_program`), então a coluna
// de expressões é a única fonte da lista.
// Uso: bun scripts/gen-options-parity.js > tests/golden/options_parity_bun.tsv
const exprs = [
  // useShadowRealm
  "ShadowRealm", "ShadowRealm.prototype.evaluate", "ShadowRealm.prototype.importValue",
  // useSharedArrayBuffer
  "SharedArrayBuffer", "SharedArrayBuffer.prototype.grow", "SharedArrayBuffer.prototype.slice", "Atomics.waitAsync", "Atomics.wait", "Atomics.notify",
  // useAtomicsPause (sempre ligada no JSC do bun)
  "Atomics.pause",
  // useTemporal
  "Temporal", "Temporal.Now", "Temporal.Instant", "Temporal.PlainDate", "Temporal.ZonedDateTime", "Temporal.Duration", "Date.prototype.toTemporalInstant",
  // useExplicitResourceManagement
  "DisposableStack", "AsyncDisposableStack", "SuppressedError", "Symbol.dispose", "Symbol.asyncDispose", "Iterator.prototype[Symbol.dispose]",
  // useIterator* e useJointIteration
  "Iterator", "Iterator.from", "Iterator.concat", "Iterator.zip", "Iterator.zipKeyed", "Iterator.prototype.chunks", "Iterator.prototype.windows",
  "Iterator.prototype.includes", "Iterator.prototype.join", "Iterator.prototype.map", "Iterator.prototype.flatMap", "Iterator.prototype.take",
  "Iterator.prototype.toAsync", "AsyncIterator",
  // Float16, Math, Array, Promise, Error, JSON, Map/Set, Uint8Array base64
  "Float16Array", "Math.f16round", "DataView.prototype.getFloat16", "Math.sumPrecise", "Math.clamp", "Math.signbit",
  "Array.fromAsync", "Array.prototype.group", "Array.prototype.toSorted", "Array.prototype.with", "Object.groupBy",
  "Promise.try", "Promise.withResolvers", "Promise.isPromise", "Promise.allKeyed", "Promise.allSettledKeyed",
  "Error.isError", "Error.captureStackTrace",
  "JSON.rawJSON", "JSON.isRawJSON",
  "Map.prototype.getOrInsert", "Map.prototype.getOrInsertComputed", "WeakMap.prototype.getOrInsert", "Set.prototype.union",
  "Uint8Array.fromBase64", "Uint8Array.prototype.toHex", "Uint8Array.prototype.setFromBase64",
  "RegExp.escape", "String.dedent", "Symbol.metadata", "Reflect.getOwnPropertyDescriptors",
  // useBigIntMathMethods
  "BigInt.sqrt", "BigInt.abs", "BigInt.asIntN",
  // Intl
  "Intl.DurationFormat", "Intl.Segmenter", "Intl.Locale.prototype.getWeekInfo", "Intl.supportedValuesOf", "Intl.NumberFormat.prototype.formatRange",
  // WebAssembly
  "WebAssembly", "WebAssembly.Memory", "WebAssembly.Suspending", "WebAssembly.promising", "WebAssembly.Memory.prototype.toFixedLengthBuffer",
  // base
  "WeakRef", "FinalizationRegistry", "AggregateError", "ArrayBuffer.prototype.transfer", "ArrayBuffer.prototype.resize",
];

// Mesmo programa que o teste em Rust monta: uma linha de resultado por expressão, em `R`.
const probeProgram = (list) =>
  "var out = [];\n" +
  list.map((e) => `try { out.push(typeof (${e})); } catch (e) { out.push("throws " + e.name); }`).join("\n") +
  '\nvar R = out.join("\\n");';

(0, eval)(probeProgram(exprs));
const results = globalThis.R.split("\n");
if (results.length !== exprs.length) throw new Error("tamanho divergente");
process.stdout.write(require("./golden-prelude.js").assertPublicResult(exprs.map((e, i) => `${e}\t${results[i]}`).join("\n") + "\n"));
process.stderr.write("expressões: " + exprs.length + "\n");
