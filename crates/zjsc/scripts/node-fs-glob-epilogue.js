  // Os despachantes de `fs.ts` e `fs.promises.ts` do bun.
  const { validateFunction } = validators;

  function arrayGlobSync(pattern, options) {
    return Array.from(globModule.globSync(pattern, options ?? kEmptyObject));
  }

  function callbackGlob(pattern, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    validateFunction(callback, "callback");
    // `process.nextTick` para que um `throw` do callback saia como exceção não capturada em vez de rejeitar a cadeia
    // interna (e nunca voltar ao `callback` como erro), como no Node.
    Array.fromAsync(globModule.glob(pattern, options ?? kEmptyObject)).then(
      result => process.nextTick(callback, null, result),
      error => process.nextTick(callback, error),
    );
  }

  async function* promisesGlob(pattern, options) {
    yield* new globModule.Glob(pattern, options).glob();
  }

  return { Glob: globModule.Glob, globSync: arrayGlobSync, glob: callbackGlob, promisesGlob, createMatcher };
