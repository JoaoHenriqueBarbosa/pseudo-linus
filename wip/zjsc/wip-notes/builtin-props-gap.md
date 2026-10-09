# Lacunas das propriedades próprias dos builtins (medido no bun 1.4.2)

Fonte da medição: `bun-builtin-props.json` (`Object.getOwnPropertyNames`, só strings, ordenada; ordem e símbolos
estão em `tests/golden/own_keys_bun.json`). Conferência por leitura do código de instalação de cada objeto em `src/runtime`, sem
executar nada. `length`, `name`, `prototype` e `constructor` foram tratados como instalados pelo
`InternalFunction::finishCreation` e pelo `install_*` de cada global (não conferidos um a um).

## Faltando no porte

| Objeto | Nome | Estado | Onde / motivo |
|---|---|---|---|
| `Date.prototype` | `toTemporalInstant` | LISTADO | Depende do porte de Temporal (`temporal*`, de outro agente); `Options::useTemporal` já é `true`. |
| `Error` | `appendStackTrace`, `prepareStackTrace` | LISTADO (não é do JSC) | Só existem no bun (`ZigGlobalObject`), não há C++ em `upstream/JavaScriptCore`. `error_natives.rs` (de outro agente) instala `stackTraceLimit`, `captureStackTrace` e `isError`. `prepareStackTrace` o porte lê só se alguém atribuir. |

## Portados nesta rodada

| Objeto | Nome | Onde |
|---|---|---|
| `Object` | `fromEntries`, `groupBy` | `object_constructor.rs` (`put_direct_builtin_function_without_transition` com `ObjectConstructorFromEntriesCode` e `ObjectConstructorGroupByCode`, como o `ObjectConstructor.cpp`) |
| `Array.prototype` | `copyWithin`, `toSpliced` | `array_prototype.rs` (`array_proto_func_copy_within`, `array_proto_func_to_spliced`, sem os atalhos `fastCopyWithin`/`fastToSpliced`) |
| `Array` | `from` | `array_constructor.rs` (público, além do privado `@from`) |
| `Array.prototype` | `sort`, `toSorted` | instalados (`sortImpl`) |
| `Math` | `sumPrecise` | `math_object.rs` mais `wtf/precise_sum.rs` (novo, registrado em `wtf/mod.rs`; acumulador exato único no lugar de `XsumSmall`/`XsumLarge`, com testes) |

## Sobrando no porte

Nada observável por `getOwnPropertyNames`. Os nomes privados (`@forEach`, `@from`, `@shift`...) não
aparecem na enumeração. `Promise.isPromise` e `Iterator`/`Math` com opções desligadas
(`useBigIntMathMethods`, `usePromiseIsPromise`, `useShadowRealm`) ficam ausentes como no bun.

## Conferido e completo

Object (menos `fromEntries`/`groupBy`, agora portados), `Object.prototype` (o `__proto__` entra por
`add_underscore_proto_accessor` no `js_global_object_init.rs`), `Array`, `String` e
`String.prototype` (as 54 funções, incluindo `anchor`...`sup`, `trimLeft`/`trimRight`),
`Number` e `Number.prototype`, `JSON` (`rawJSON`/`isRawJSON` atrás de `useJSONSourceTextAccess`, `true`),
`Reflect` (13, as quatro `JSBuiltin` aqui são nativas equivalentes, ver cabeçalho de `reflect_object.rs`),
`Promise` (`try`, `withResolvers` etc.) e `Promise.prototype`, `Map` (`groupBy`) e `Map.prototype`
(`getOrInsert`, `getOrInsertComputed`), `Set` e `Set.prototype` (os sete métodos de conjunto),
`Symbol` (16 conhecidos mais `for`/`keyFor`) e `Symbol.prototype`, `Date` e `Date.prototype`
(menos `toTemporalInstant`), `RegExp` (`escape`, 21 acessores legados) e `RegExp.prototype`, `BigInt` e
`BigInt.prototype`, `Iterator` (`from`, `concat`, `zip`, `zipKeyed`) e `Iterator.prototype`
(`chunks`, `windows`, `includes`, `join` atrás de `Options`, todas `true`), `ArrayBuffer` e
`ArrayBuffer.prototype`, `Atomics` (14, `waitAsync` incluso).

## Sem conferir

- Se o valor das `Options` que gateiam nomes (`useJSONSourceTextAccess`, `useIteratorChunking`,
  `useJointIteration` etc.) chega ao `Options::with` em runtime igual ao default de `options_list.rs`.
- Ordem, Symbols e atributos (`writable`/`enumerable`/`configurable`, `length`, `name`): agora medidos sem ordenar em `tests/golden/own_keys_bun.json` (`scripts/gen-own-keys-golden.js`) e comparados por `builtin_own_keys_golden.rs`; falta rodar e fechar o que o teste acusar.
- `Error.prototype` (`constructor`, `message`, `name`, `toString`): criação em `error_natives.rs`
  (de outro agente), `toString` não foi localizado por grep.
- Os arquivos novos e editados não foram compilados nem testados.
