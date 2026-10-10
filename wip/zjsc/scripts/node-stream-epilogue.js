
  // Os módulos que o bun resolve por dentro (`internal/*`) e que aqui vêm do `node_internal.js`.
  const provided = {
    "internal/validators": validators,
    "internal/errors": { aggregateTwoErrors },
    "internal/shared": { once, kEmptyObject, kWeakHandler, kAutoDestroyed, resistStopPropagation },
    "internal/abort_listener": { addAbortListener },
    "internal/primordials": { SafeSet, SafeArrayIterator },
    // Nenhum hook de `async_hooks` está ligado (o módulo ainda não existe), então o `eos` não embrulha o callback.
    "internal/async_hooks": { enabledHooksExist: () => false },
    "node:util/types": { isArrayBufferView: ArrayBuffer.isView, isUint8Array: value => value instanceof Uint8Array },
  };

  const loaded = { __proto__: null };
  function require(name) {
    if (name in provided) return provided[name];
    if (name in loaded) return loaded[name].exports;
    const define = definitions[name];
    if (define === undefined) return host.requireBuiltin(name);
    const module = { exports: {} };
    loaded[name] = module;
    define(module, undefined, require);
    return module.exports;
  }

  return require("node:stream");
})
