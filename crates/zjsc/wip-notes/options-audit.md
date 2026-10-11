# Auditoria de Options contra o bun 1.4.2

## Pontos de entrada

Todos criam o realm por `new_global_object()` (`src/api/eval.rs`), que chama `apply_bun_options()` antes de
`VM::new()`: `evaluate_script`, `evaluate_script_running_timers`, `evaluate_named_script_result`,
`evaluate_indirect_eval` e `evaluate_module_map` / `evaluate_module` (`src/api/module.rs`). O único outro `VM::new()
` fora de `runtime/` está em testes de `yarr_matching_context_holder.rs`, sem realm de script. Nenhuma divergência de
caminho encontrada, nenhuma correção de código necessária.

## Flags de `Options` e o bun (medido com typeof)

Ligadas pelo bun (já aplicadas em `src/api/bun_options.rs`): `useSharedArrayBuffer`, `useShadowRealm`, `useTemporal`,
`useExplicitResourceManagement`, `useImportDefer`, `useIteratorChunking/Includes/Join/Sequencing`.
Já `true` por default no upstream e coerentes com o bun: `useJointIteration` (`Iterator.zip`, `zipKeyed`
existem), `useJSONSourceTextAccess` (`JSON.rawJSON`), `useAsyncStackTrace`, `useWasm`.

Desligadas no bun e desligadas no porte (default do upstream `false`): `useBigIntMathMethods` (`BigInt.sqrt`
undefined), `usePromiseIsPromise` (`Promise.isPromise` undefined), `useMoreCurrencyDisplayChoices`,
`useRegExpBufferBoundaries`, `useWasmJSTypes`, `useWasmWideArithmetic`.

APIs que o bun não expõe e que o porte também não pode expor: `Math.clamp`, `Math.signbit`, `Array.prototype.group`,
`Promise.allKeyed`, `String.dedent`, `Symbol.metadata`, `Iterator.prototype.toAsync`, `AsyncIterator`.
APIs que o bun expõe sem flag própria e que o porte precisa ter: `Math.sumPrecise`, `Atomics.pause`, `Float16Array`,
`Intl.DurationFormat`, `Map.prototype.getOrInsert*`, `Uint8Array.fromBase64`, `RegExp.escape`, `Error.isError`.

Anotação: o bun recusa `WebAssembly.Memory({address:"i64"})` ("requires Memory64 to be enabled"), enquanto o porte
tem `useWasmMemory64` true por default. Resolvido: `apply_bun_options` desliga `useWasmMemory64`, e o parser de seções
(`wasm_section_parser.rs`) recusa memória 64-bit com `Memory64 is not enabled`, como o bun
(`CompileError: WebAssembly.Module doesn't parse at byte 12: Memory64 is not enabled`).

## Teste

`scripts/gen-options-parity.js` mede 87 expressões no bun e escreve `tests/golden/options_parity_bun.tsv`.
`tests/options_bun_parity.rs` roda a mesma lista em `evaluate_script`, `evaluate_named_script_result` e
`evaluate_module_map`. Não rodado aqui (sem cargo); a primeira execução pode revelar APIs ainda não portadas
(candidatas: `Math.sumPrecise`, `Iterator.zip`, `WebAssembly.Suspending`, `Uint8Array.prototype.setFromBase64`), que
são lacunas de porte, não de opções.
