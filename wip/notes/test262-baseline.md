# Linha de base do test262 sobre o zjsc

test262 no commit `2e0a56762801e275a9fdf96dc49d90ba0cddcf63`. Medido com `scripts/test262-run.py` e o `examples/test262_worker.rs`.

Total: 52017 testes, passaram 50706 (97.5%), falharam 1311.

## Por diretório de topo

| diretório | testes | passou | % |
|---|---|---|---|
| annexB | 1086 | 1049 | 96.6 |
| built-ins | 23823 | 23335 | 98.0 |
| intl402 | 3382 | 3240 | 95.8 |
| language | 23726 | 23082 | 97.3 |

## Maiores subdiretórios com falha

| diretório | testes | falharam | % que passa |
|---|---|---|---|
| language/expressions | 11102 | 454 | 95.9 |
| built-ins/Atomics | 389 | 123 | 68.4 |
| language/module-code | 599 | 109 | 81.8 |
| built-ins/Promise | 732 | 88 | 88.0 |
| built-ins/Error | 93 | 38 | 59.1 |
| built-ins/Proxy | 311 | 38 | 87.8 |
| intl402/DateTimeFormat | 245 | 38 | 84.5 |
| language/import | 129 | 34 | 73.6 |
| built-ins/TypedArrayConstructors | 738 | 30 | 95.9 |
| intl402/Temporal | 2029 | 30 | 98.5 |
| built-ins/ArrayBuffer | 221 | 28 | 87.3 |
| intl402/Locale | 190 | 26 | 86.3 |
| language/statements | 9347 | 23 | 99.8 |
| annexB/language | 845 | 19 | 97.8 |
| built-ins/Function | 509 | 19 | 96.3 |
| intl402/Intl | 66 | 19 | 71.2 |
| annexB/built-ins | 241 | 18 | 92.5 |
| built-ins/Symbol | 98 | 17 | 82.7 |
| built-ins/Array | 3083 | 16 | 99.5 |
| built-ins/DataView | 561 | 13 | 97.7 |
| built-ins/RegExp | 1879 | 13 | 99.3 |
| built-ins/TypedArray | 1453 | 11 | 99.2 |
| intl402/NumberFormat | 251 | 11 | 95.6 |
| language/global-code | 42 | 10 | 76.2 |
| language/eval-code | 347 | 9 | 97.4 |

## As 10 maiores causas de falha

Agrupadas pela primeira causa de cada teste, com números e textos entre aspas normalizados.

| # | testes | causa | exemplo |
|---|---|---|---|
| 1 | 220 | `Test262:AsyncTestFailure:ResolveMessage: Cannot find module 'X' imported from /test.js` | language/expressions/dynamic-import/assignment-expression/additive-expr.js |
| 2 | 201 | `TypeError: $N.createRealm is not a function. (In 'X', 'X' is undefined)` | annexB/built-ins/RegExp/legacy-accessors/input/this-cross-realm-constructor.js |
| 3 | 136 | `TypeError: undefined is not an object (evaluating 'X')` | built-ins/ArrayBuffer/prototype/immutable/this-has-no-arraybufferdata-internal.js |
| 4 | 103 | `ResolveMessage: Cannot find module 'X' imported from main.mjs` | built-ins/Proxy/preventExtensions/trap-is-undefined-target-is-proxy.js |
| 5 | 90 | `SyntaxError: Unexpected identifier 'X'. 'X' can only be followed with meta or defer.` | language/expressions/dynamic-import/catch/nested-arrow-import-catch-import-source-source-text-module.js |
| 6 | 65 | `Test262:AsyncTestFailure:Test262Error: Test262Error: Expected SameValue('X', 'X') to be true` | language/expressions/dynamic-import/catch/nested-arrow-import-catch-eval-rqstd-abrupt-typeerror.js |
| 7 | 34 | `Test262Error: Expected SameValue('X', 'X') to be true` | annexB/built-ins/RegExp/prototype/Symbol.split/Symbol.match-getter-recompiles-source.js |
| 8 | 32 | `TypeError: undefined is not an object (evaluating 'X'stack'X')` | built-ins/Error/prototype/stack/getter-error-instance.js |
| 9 | 29 | `assíncrono: não terminou (sem Test262:AsyncTestComplete)` | language/expressions/dynamic-import/catch/nested-arrow-import-catch-eval-script-code-target.js |
| 10 | 21 | `SyntaxError: Invalid character: 'X'` | language/comments/hashbang/module.js |

## Leitura da medição

Primeira falha de cada um dos 1311 testes que não passam, por natureza:

| natureza | testes |
|---|---|
| módulo não resolvido: o corredor não tem sistema de arquivos para os `_FIXTURE.js` (`import()` em script e `import` em módulo) | 353 |
| `$262.createRealm` ausente (o corredor não o fornece) | 201 |
| sintaxe `import source` e `import defer` (proposta ainda não portada) | 104 |
| crash do processo | 8 |
| tempo limite de 20 s | 2 |
| demais (comportamento do motor, ver as 10 causas acima e `Atomics`, `Error.prototype.stack`, `module-code`) | 643 |

Os 353 e os 201 são limites do corredor (e do `$262`), não do motor: o número real do motor é maior que 97,5%. Fechá-los é trabalho de
corredor (um `ModuleFs` em memória com os fixtures da pasta do teste, e um `createRealm` que crie outro global).

## Depois do corredor (createRealm real, fixtures em memória, harness global nos módulos)

Total: 51285 de 52017 (98,6%), contra 50706 (97,5%) na primeira medição. Dos 1311 falhos antes, 581 passam.

| Topo | Passou | Total | % |
|---|---|---|---|
| built-ins | 23472 | 23823 | 98,5 |
| language | 23511 | 23726 | 99,1 |
| annexB | 1055 | 1086 | 97,1 |
| intl402 | 3247 | 3382 | 96,0 |

Restos mais frequentes (falta do motor, não do corredor): `$262.agent` (112), `import source/defer` (90),
`Promise.allKeyed`/`allSettledKeyed` (39), ArrayBuffer imutável (`transferToImmutable`, ~50), async de módulo que não termina (30).

## Como reproduzir

Compilar o worker (`cargo build --example test262_worker` em `wip/zjsc`) e, no container com o test262 em `/t262`, o worker em
`/target/debug/examples/test262_worker`:

```
python3 wip/zjsc/scripts/test262-run.py --test262 /t262 --worker /target/debug/examples/test262_worker \
    --out /out/full.jsonl --jobs 16 --timeout 20
python3 wip/zjsc/scripts/test262-run.py --summary /out/full.jsonl --report wip/notes/test262-baseline.md --commit <hash do test262>
```

Rodada completa de 52017 testes (sloppy e strict por padrão): cerca de 16 minutos com 16 processos. Os resultados por teste
(`full.jsonl`) são gerados em massa e não se versionam. `test/staging` e `test/harness` ficaram de fora.
Modos: `sloppy` e `strict` por padrão, só `strict` com `onlyStrict`, só `sloppy` com `noStrict` e `raw`, `module` com a flag; o teste
passa quando passa em todos os modos. O veredicto dos `negative` confere o tipo da exceção e a fase (um marcador impresso antes do
harness distingue erro de análise e ligação de erro de execução).
