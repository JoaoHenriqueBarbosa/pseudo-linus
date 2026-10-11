# Auditoria de async: Promise, async/await, geradores, iterator helpers

Data: 2026-10-08. Sem cargo nem build; só leitura contra o golden medido no bun 1.4.2.

## O que foi criado

- `scripts/gen-async-golden.js`: 760 programas (then/catch/finally por origem x manipulador, interleaving
  de cadeias, ticks de resolve com promise e thenable, combinadores com 12 tipos de entrada, async/await,
  geradores, async geradores, for-await e asyncFromSyncIterator, iterator helpers, queueMicrotask,
  subclasses de Promise). Cada programa roda num processo bun próprio, com timeout de 10 s, e registra
  eventos no array global `log`.
- `tests/golden/async_bun_harness.js`: auxiliares `L`, `tick(n, rótulo)`, `thenable(v, rótulo)`,
  `__run(fonte)` (eval indireto) e `__final()` (JSON do log, ou `error<TAB>name<TAB>message`).
- `tests/golden/async_bun.tsv`: o golden gerado (sem caminhos da máquina, sem travessão).
- `tests/async_bun_golden.rs`: teste `async_programs_match_bun`, um realm novo por programa, usa
  `evaluate_named_script_result(.., "__final()")`, que esvazia as microtarefas antes de ler o resultado.
  Exige pelo menos 400 programas e lista todas as divergências de uma vez.

Rejeição não tratada: o bun recebe `process.on("unhandledRejection", ...)` vazio; os programas só
exercitam o caso de handler anexado tarde (`p.catch` dentro de uma microtarefa posterior).

## Leitura do código (sem rodar)

`js_promise.rs` (`resolve_promise`, `resolve_with_internal_microtask*`, `trigger_promise_reactions`) e
`js_microtask_async.rs` são portes diretos do JSC/Bun, com as mesmas tarefas internas
(`PromiseResolveThenableJob[Fast]`, `...WithInternalMicrotask`), então a contagem de ticks do await e do
resolve com promise/thenable deve bater. Mensagens conferidas por grep contra o golden e presentes:
take/drop (NaN e intervalo), "Iterator result interface is not an object", "Promise resolve is not a
function", "|this| should be an async generator", "executor did not take a resolve function",
"calling Iterator constructor without new is invalid", "Iterator cannot be constructed directly",
"Cannot call a constructor Promise without |new|" (vem do bytecode generator, `ConstructorKind::Naked`).

## Divergências

Nenhuma encontrada por leitura, nenhuma correção aplicada em `src/runtime`. Pontos de maior risco para a
primeira execução do teste (conferir primeiro quando o build estiver livre):

1. Ordem de ticks em `for await` sobre iterador síncrono e em `yield*` de async geradores (casos
   `tick(n, ...)` das seções 7 e 8).
2. `Promise.all(1)` e `Promise.all({})`: o bun devolve `TypeError: Type error`, que no zjsc sai de
   `iterator_operations.rs` (mensagens "Type error" em `get_iterator`).
3. `Iterator.prototype[Symbol.toStringTag] = 'X'` deve lançar "SetterThatIgnoresPrototypeProperties was
   called on a home object." (existe em `iterator_prototype.rs`).
4. `Promise.all()` sem argumento: mensagem "undefined is not an object (evaluating 'Promise.all()')",
   que depende do texto de avaliação do call site.
5. Mensagem de `queueMicrotask(5)` do Node-compat do bun (`The "callback" argument must be of type
   function...`): se o zjsc não tem `queueMicrotask` com essa mensagem, é divergência de ambiente do bun,
   não do JSC; remover do golden se for o caso.

## Golden focado em Promise e microtarefas (2026-10-08)

`tests/golden/promise_bun.tsv` (1200 programas, `scripts/gen-promise-golden.js`, bun 1.4.2, harness de
`tests/golden/async_bun_harness.js`, teste em `tests/promise_bun_golden.rs`). O gerador produz 2329
programas por combinação e fica com uma amostra uniforme e determinística de 1200 (conferido: duas
execuções dão o mesmo arquivo; sem caminho da máquina; 70 casos terminam em `error`, mensagem do bun).
Famílias: resolve/reject por 20 tipos de valor (thenable que lança, que retorna thenable, getter de `then`
que lança, subclasse, promessa nativa) medidos em 1 a 6 ticks, await e async function (3 ticks),
`all/allSettled/any/race` com 17 formas de entrada, `withResolvers`/`try` (guardados por existência),
`finally` (valor de passagem, species, `then` observável), species e construtor customizado em `then`,
identidade de `Promise.resolve(p)`, executor lançando depois de resolver, rejeição com tratador tardio,
async geradores (fila de `next`, `return()` em suspenso, `throw`, `yield*`, finally com await), `for await`
sobre iteradores sync e async, e patches de `Promise.prototype.then`/`Promise.resolve` observando o que
await/async geradores consultam. Fora: `queueMicrotask` e timers (do host).

Leitura de `src/runtime/promise_prototype.rs` contra o upstream: em `upstream/` só existe
`builtins/PromiseConstructor.js` (`try` e o construtor); `PromisePrototype.js` e
`AsyncGeneratorPrototype.js` não existem aqui, o porte é nativo (como no JSC do bun). `then`, `catch` e
`finally` (caminho rápido, species watchpoint, caminho lento com `then(onFinally, onFinally)` quando não
chamável) batem com a especificação e com as mensagens do golden. Nenhuma divergência óbvia encontrada,
nenhuma edição em `src/runtime`. Não rodei cargo: o resultado do teste novo ainda é desconhecido.

## Segunda passada: contagem de ticks por leitura (2026-10-08)

Só leitura, sem cargo. Conferido contra a especificação, o upstream em `upstream/JavaScriptCore` e as linhas
do `promise_bun.tsv`. Nenhuma divergência, nenhuma edição.

- Await (`resolve_with_internal_microtask_for_async_await`, `js_promise.rs`): promessa nativa do realm com
  watchpoint de species válido ou `constructor === Promise` entra direto como reação (1 tick); constructor
  que lança resume de forma síncrona com rejeição (igual ao `PromiseResolve` abrupto da especificação);
  thenable não nativo passa por `PromiseResolveThenableJobWithInternalMicrotask` (job + then + reação, 3
  ticks, e o getter de `then` é lido na hora do resolve, como no golden `["get","errgt"]`). Async function
  que devolve promessa: job de thenable (tick 1), reação que resolve a externa (tick 2), tratador externo
  (tick 3), batendo com os 3 ticks do golden.
- `is_definitely_non_thenable` e `Structure::add` (`has_special_properties` para a chave `then`) batem com
  `JSPromise.cpp` e `StructureInlines.h`. Falta só o cache `definitelyNonThenableState` do upstream, que é
  otimização sem efeito observável.
- Async generator (`js_microtask_async.rs`, `async_generator_prototype.rs`): `next/return/throw`, fila,
  `draining-queue`, `AsyncGeneratorAwaitReturn` e `AsyncGeneratorUnwrapYieldResumption` seguem o texto do
  ES2025 passo a passo. `yield x` faz Await antes do `CompleteStep` (razão `Yield`), `yield*` entrega sem
  Await (`YieldNoAwait`), `return x` faz Await no corpo (`ReturnNode`, `AsyncGeneratorBodyMode`), igual ao
  upstream.
- `emit_delegate_yield` (`bytecode_generator_cpp6.rs`) tem os mesmos pontos de `emitAwait` que
  `BytecodeGenerator.cpp:5526` (resultado de `next/throw/return`, valor de `return` sem método, close com
  await).
- `AsyncFromSyncIterator` (`async_from_sync_iterator_prototype.rs`): 1 tick no await do valor + 1 do
  consumidor por iteração (2 por volta de `for await` sobre iterador sync); fecha o iterador síncrono quando
  o valor rejeita com `done` falso, exceto em `return`; `throw` ausente fecha e lança TypeError.
- `finally` (`promise_prototype.rs`, `js_microtask.rs`): o caminho rápido do bun (`PromiseFinallyReactionJob`)
  liquida a derivada já no tick 1 quando `onFinally` devolve não promessa (2 ticks a menos que a
  especificação), e em 2 ticks quando devolve promessa nativa. É a medida do bun e o golden é coerente com
  isso (`finally(() => Promise.resolve('fp'))` com `outer` empatado em `t3`); o caminho lento segue a
  especificação (species, `then(thunk)`, não chamável repassa `onFinally` nos dois lados).

Simulações à mão (10 por família, resumidas): await de valor, de promessa, de thenable e de thenable com
getter que lança; async function com `return promise`; `yield 1; yield 2; return 3` com 4 `next()` na fila;
`return()` em `suspended-start` seguido de `next()`; `throw()` em `suspended-start`; `for await` sobre
async gerador com 3 e 6 e 9 ticks (`v1,v2,t3,done`); `finally` fulfilled e rejected com valor, thenable e
promessa. Todas coerentes com a contagem acima. Não simulados por falta de tempo: `Promise.any` e
`allSettled` com entradas mistas e `yield*` sobre async iterador que troca de realm.

## Golden de ordem de microtarefas (2026-10-08)

- `scripts/gen-microtask-golden.js` gera `tests/golden/microtask_bun.tsv` (500 programas de 960 candidatos,
  amostra uniforme, descartando o que já está em `promise_bun.tsv` e `async_bun.tsv`); o teste é
  `tests/microtask_bun_golden.rs`, mesmo harness de `async_bun_harness.js`. Ainda NÃO foi rodado contra o
  zjsc (sem cargo nesta rodada): a primeira execução diz quais linhas divergem.
- Famílias: await de 5 operandos (valor, nativa, thenable, rejeitada, getter de `then` com efeito) em 10
  formas (function, arrow, método, async generator, `return await` vs `return`, `yield await` vs `yield`,
  `return` de gerador, `await` em `finally`) com marcadores `tick(1..5)`; pares de funções intercaladas;
  fila de requests do async generator (next/return/throw com valor, Promise, thenable e rejeição) em
  suspendedStart, suspendedYield, executing e completed; `yield*` e `for await` sobre iterador sync, async,
  com Promise nos valores, com `return` ausente, inválido, que lança ou rejeita (break, return, throw,
  continue com label); `Promise.all/allSettled/any/race` com 10 tipos de item, subclasse, `resolve` e
  `then` sobrescritos; `finally` com 8 formas de callback; species; `Promise.prototype.then` remendado.
- Fora de escopo: `queueMicrotask` e `process.nextTick` existem no bun mas são do host (o JSC puro não
  tem), timers e unhandled rejection idem.
- Nenhuma correção de contagem de ticks foi feita: sem execução não há divergência medida, e a leitura do
  código (seção acima) já bate com a especificação nos pontos listados.
