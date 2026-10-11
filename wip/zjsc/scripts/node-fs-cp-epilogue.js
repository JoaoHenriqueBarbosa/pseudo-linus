  // Os despachantes de `fs.cpSync` (`fs.ts`) e `fs.promises.cp` (`fs.promises.ts`) do bun, sem o caminho nativo: o
  // bun só o usa quando `ok` e o resultado é o mesmo do caminho em JavaScript, que aqui roda sempre (com o `checked`
  // da validação, como no bun, para a ordem dos erros ser a mesma).
  const { getValidatedFsPath } = validators;

  const { validateCpOptions, cpSyncFn, tryNativeFastPathSync } = cpSyncModule;
  const { cpFn, tryNativeFastPath } = cpModule;

  const isPlainCopy = options =>
    !options.filter && !options.dereference && !options.preserveTimestamps && !options.verbatimSymlinks && !options.mode && !options.errorOnExist && options.force;

  function cpSync(src, dest, options) {
    options = validateCpOptions(options);
    src = getValidatedFsPath(src, "src");
    dest = getValidatedFsPath(dest, "dest");
    if (isPlainCopy(options)) return cpSyncFn(src, dest, options, tryNativeFastPathSync(src, dest, options).checked);
    return cpSyncFn(src, dest, options);
  }

  async function cp(src, dest, options) {
    options = validateCpOptions(options);
    src = getValidatedFsPath(src, "src");
    dest = getValidatedFsPath(dest, "dest");
    if (isPlainCopy(options)) return cpFn(src, dest, options, (await tryNativeFastPath(src, dest, options)).checked);
    return cpFn(src, dest, options);
  }

  return { cpSync, cp, validateCpOptions, getValidatedFsPath };
