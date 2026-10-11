# Quadro de recursos recentes do ECMAScript: bun 1.4.2 contra o porte

Medido em 2026-10-08 (`bun -e` com `typeof`/`eval` para cada nome) e por `grep -rl` em `src` e `tests` (número de
arquivos que citam o nome). "golden" = algum `tests/*` ou `tests/golden/*` cita o nome; contagem baixa não prova
ausência de cobertura, só aponta onde olhar. Nada foi compilado nem executado no porte.

## Existe no bun, existe no porte, tem golden

`Object.groupBy`, `Map.groupBy`, `Promise.withResolvers`, `Promise.try`, `Array.fromAsync`, `findLast`/`findLastIndex`,
`with`/`toSorted`/`toSpliced`/`toReversed`, `isWellFormed`/`toWellFormed`, `Atomics.waitAsync`, `Atomics.pause`,
`ArrayBuffer.prototype.transfer`/`transferToFixedLength`/`resize`, `Intl.DurationFormat`, `Intl.Locale.prototype.getWeekInfo`
(e `getTextInfo`), `Error.captureStackTrace`, `Error.isError`, `Math.sumPrecise`, `Math.f16round`, `Float16Array`,
`Iterator.prototype.*`, `Iterator.from`, `Iterator.zip`/`zipKeyed`/`concat`, `RegExp.escape`, `Uint8Array.fromBase64`/`fromHex`/`toHex`/`toBase64`/`setFromBase64`,
métodos de `Set` (`union`... `isSubsetOf`), `Map.prototype.getOrInsert`/`getOrInsertComputed`, `Symbol.dispose`/`asyncDispose`,
`DisposableStack`/`AsyncDisposableStack`/`SuppressedError`, `using`/`await using`, `JSON.rawJSON`/`isRawJSON`, `Temporal.*`,
`Date.prototype.toTemporalInstant`, `WeakRef`, `FinalizationRegistry`, `Intl.Segmenter`, `Object.hasOwn`, `String.prototype.at`/`replaceAll`/`matchAll`,
`Symbol.prototype.description`, `Error` `cause`, blocos estáticos de classe, hashbang, `await` no topo, índices de regexp (`/d`),
`??=`, separador numérico, flag `v`, modificadores `(?i:...)`, grupos nomeados duplicados, `import.defer(...)`, `import ... with {}`.

Cobertura mais fina (poucos arquivos citam o nome): `Error.isError` (1 arquivo de teste, mais `error_bun.tsv`), `Iterator.zip`/`concat` (2),
`DisposableStack` (2), `import` com atributos (1), `Atomics.pause` (3).

## Existe no bun, existe no porte, SEM golden

| Recurso | Porte | Observação |
|---|---|---|
| `ShadowRealm` (`new`, `evaluate`, `importValue`, funções remotas) | `shadow_realm_*.rs`, `js_remote_function.rs`, builtins `@evalInRealm`/`@importInRealm`/`@moveFunctionToRealm` | 0 arquivos em `tests`. Medido no bun: `evaluate("1+1")` devolve 2; `({})` lança `TypeError: value passing between realms must be callable or primitive`; `throw` dentro do realm vira `TypeError` com a mensagem do valor; `ShadowRealm()` sem `new`: `calling ShadowRealm constructor without new is invalid`; `evaluate(1)`: `` `%ShadowRealm%.evaluate requires that the |sourceText| argument be a string ``. As mensagens do porte já coincidem por leitura; falta o gerador `scripts/gen-*-golden.js` e o teste. |

## Não existe no bun (fora do escopo; sem golden por desenho)

`Math.clamp`, `Promise.allKeyed`, `Symbol.metadata`, `String.dedent`, `Array.isTemplateObject`, `Array.prototype.group`/`lastItem` (legado),
decorators (`@x`), `accessor` em campo de classe (SyntaxError no bun), `import.source`, `Intl.Locale.prototype.weekInfo` (só o método `getWeekInfo`).

## Existe no bun, falta no porte

Nenhuma lacuna encontrada nos nomes medidos. `structuredClone` existe no bun mas é do WebCore/Bun, não do JavaScriptCore
(`src` não tem, e não deve ter).

## Golden dedicado aos recursos recentes (2026-10-08)

`scripts/gen-recent-features-golden.js` (medido no bun 1.4.2) gera `tests/golden/recent_features_bun.tsv` (581 programas) e
`tests/recent_features_bun_golden.rs` o consome. Cobre `typeof`, `name`/`length` e descritores de cada API da lista acima,
chaves próprias dos protótipos, mensagens de erro exatas (inclusive `... requires that |this| be a pending DisposableStack object`),
base64/hex, `Math.sumPrecise`/`f16round`, `Error.isError`, `SuppressedError`, `Promise.try`/`withResolvers`, `Array.fromAsync`,
`Object.groupBy`, helpers e `concat`/`zip` de `Iterator`, cópias de Array e TypedArray, `isWellFormed`, `Atomics.waitAsync`/`pause`,
`RegExp.escape`, `JSON.rawJSON`, `Float16Array`, `DataView` Float16, `Intl.Locale` (23 locales, getters e `get*`),
`DisposableStack`/`AsyncDisposableStack`, `using`/`await using` (blocos, laços, geradores, erros combinados), `Temporal.Instant`/`Now`
e `ArrayBuffer` transfer/resize/detached. Nada foi compilado nem executado no porte: a primeira rodada do teste dirá as divergências.
Lacunas de nome no porte: nenhuma (todos os métodos acima aparecem em `src`; `grep -rl` por nome, de 1 a 27 arquivos).

## Próximo passo sugerido

Gerar `tests/golden/shadow_realm_bun.tsv` (programa `\t` valor de `R`, como `error_bun.tsv`) com o gerador no bun e o teste
`tests/shadow_realm_bun_golden.rs`, nos moldes de `tests/error_bun_golden.rs`.
