  // `fs.promises.watch` (`fs.promises.ts` do bun) sobre o mesmo `binding.watch`: um iterador assíncrono que enfileira os
  // eventos do observador.
  function promisesWatch(filename, options = {}) {
    if (filename instanceof URL) {
      throw new TypeError("Watch URLs are not supported yet");
    } else if (Buffer.isBuffer(filename)) {
      filename = filename.toString();
    } else if (typeof filename !== "string") {
      throw codes.INVALID_ARG_TYPE("filename", ["string", "Buffer", "URL"], filename);
    }
    let nextEventResolve = null;
    if (typeof options === "string") options = { encoding: options };
    const queue = [];
    const ignoreMatcher = watchModule.createIgnoreMatcher(options?.ignore);
    const signal = options?.signal;
    validators.validateAbortSignal(signal, "options.signal");
    const makeAbortError = () => host.internal.makeAbortError(undefined, { cause: signal.reason });
    const wake = () => {
      if (!nextEventResolve) return;
      const resolve = nextEventResolve;
      nextEventResolve = null;
      resolve();
    };

    // O Node nunca abre o observador quando o sinal já está abortado (o gerador lança no primeiro `next()`).
    if (signal?.aborted) {
      return {
        [Symbol.asyncIterator]() {
          let closed = false;
          return {
            async next() {
              if (closed) return { value: undefined, done: true };
              closed = true;
              throw makeAbortError();
            },
            return() {
              closed = true;
              return { value: undefined, done: true };
            },
          };
        },
      };
    }

    const watcher = binding.watch(filename, options || {}, (eventType, name) => {
      if (eventType !== "close" && eventType !== "error" && name != null && ignoreMatcher?.(name)) return;
      queue.push({ __proto__: null, eventType, filename: name });
      wake();
    });

    function onAbort() {
      watcher.close();
      wake();
    }
    signal?.addEventListener("abort", onAbort, { once: true });
    const removeAbortListener = () => signal?.removeEventListener("abort", onAbort);

    return {
      [Symbol.asyncIterator]() {
        let closed = false;
        return {
          async next() {
            while (!closed) {
              if (signal?.aborted) {
                closed = true;
                throw makeAbortError();
              }
              let event;
              while ((event = queue.shift())) {
                if (event.eventType === "close") {
                  closed = true;
                  removeAbortListener();
                  return { value: undefined, done: true };
                }
                if (event.eventType === "error") {
                  closed = true;
                  watcher.close();
                  removeAbortListener();
                  throw event.filename;
                }
                return { value: event, done: false };
              }
              const { promise, resolve } = Promise.withResolvers();
              nextEventResolve = resolve;
              await promise;
            }
            return { value: undefined, done: true };
          },

          return() {
            if (!closed) {
              watcher.close();
              closed = true;
              removeAbortListener();
              wake();
            }
            return { value: undefined, done: true };
          },
        };
      },
    };
  }

  return { watch: watchModule.watch, watchFile: watchFileModule.watchFile, unwatchFile: watchFileModule.unwatchFile, promisesWatch };
