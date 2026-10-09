# Auditoria de reflexão (Proxy, Reflect, Object.*, Symbol, WeakRef, JSON)

Data: 2026-10-08. Escopo: golden contra o bun 1.4.2 e leitura de `proxy_object.rs`, `reflect_object.rs`
e `object_constructor.rs`.

## Artefatos

- `scripts/gen-reflection-golden.js`: gera `tests/golden/reflection_bun.tsv` (1751 programas, cada um num
  processo bun próprio, com timeout; resultados com caminho da máquina são descartados). Duas execuções
  seguidas produzem o mesmo arquivo.
- `tests/golden/reflection_bun_harness.js`: o harness comum ao gerador e ao teste. Serializa o valor de
  conclusão (primitivos, arrays, objetos pelos descritores próprios, funções como `[fn nome]`) ou o erro
  (`error`, name, message JSON).
- `tests/reflection_bun_golden.rs`: `reflection_matches_bun`, no padrão de `errors_bun_golden.rs`, cada
  programa em `evaluate_script` novo (realm novo). Não foi executado (regra da tarefa: sem cargo).

Cobertura: as 13 traps (ordem, argumentos, `this`, trap não callable, trap que lança), invariantes de cada
trap com a mensagem, `Proxy.revocable` em todas as operações, proxy de função, array, classe, `Map`, `Date`,
typed array, proxy como protótipo, `ownKeys` duplicado, `getOwnPropertyDescriptor` inconsistente, as 13
funções de `Reflect` (incluindo `construct` com `newTarget`), `Object.defineProperty/defineProperties/
getOwnPropertyDescriptors/entries/values/fromEntries/groupBy/freeze/seal/isFrozen` em 16 tipos de objeto
exótico e em proxies com log de traps, `__proto__`, `__defineGetter__` e família, Symbol (well-knowns,
`description`, registry, protocolos), WeakRef, FinalizationRegistry, `JSON.stringify/parse` com proxies,
`toJSON`, replacer, reviver, `rawJSON`.

## Leitura (sem divergência encontrada)

Nenhum Edit em `src/runtime` foi necessário. O que foi conferido:

- Todas as mensagens de `TypeError` de Proxy que o bun produz nos 1751 programas (54 distintas) existem
  textualmente em `proxy_object.rs`, `proxy_constructor.rs` ou `array_constructor.rs` (revoked). As que
  parecem ausentes numa busca ingênua (`'{name}' property of a Proxy's handler should be callable`,
  `calling ... constructor without new is invalid`) são montadas por formatação.
- `perform_internal_method_get_own_property`, `perform_get_own_property_names` e `perform_has_property`
  seguem a ordem do `ProxyObject.cpp` (invariantes de `ownKeys` por chaves não configuráveis, depois
  configuráveis em alvo não extensível, depois sobras).
- `reflect_object.rs`: ordem de validação de `construct` (alvo, newTarget, array-like) e mensagens iguais às
  do bun; `set` com receiver só quando `argumentCount >= 4`.
- `object_constructor.rs`: `defineProperties` converte todos os descritores antes de definir; `freeze` e
  `seal` fazem `preventExtensions`, `ownKeys` e depois `getOwnPropertyDescriptor` (só freeze) e
  `defineProperty` por chave, na ordem do spec; mensagens de `Unable to prevent extension in Object.*`
  presentes.
- Mensagens de `JSON.parse` estão em `literal_parser.rs`.

## Pontos para olhar quando o teste rodar (hipóteses, não confirmados)

1. Mensagem de chamada de proxy não callable: o bun diz `p is not a function. (In 'p()', 'p' is an instance
   of ProxyObject)`; o caminho é o de `exception_helpers.rs` e depende de `class_name` do proxy ser
   `ProxyObject`, o que `proxy_object.rs` declara.
2. `Object.groupBy`/`Map.groupBy` (mensagens `requires that the first argument not be null or undefined`)
   moram em builtin JS: conferir quando o teste rodar.
3. `WeakRef`/`FinalizationRegistry` com símbolo registrado (`Symbol.for`): o bun rejeita com
   `should be an object or a non-registered symbol`.
4. Programas que redefinem `Object.prototype.toJSON`, `BigInt.prototype.toJSON` e afins só valem por
   realm novo: o teste já roda cada linha isolada.
