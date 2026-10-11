// Preload da sonda de `cjsRuntimeBody` (scripts/golden-prelude.js): devolve o texto da função de wrapper CJS que o runtime
// do bun montou para o arquivo (`arguments.callee.toString()`) e encerra antes de o programa executar.
globalThis.__CJS_PROBE__ = (text) => {
  require("fs").writeSync(1, "\u0001" + JSON.stringify(text) + "\n");
  process.exit(0);
};
