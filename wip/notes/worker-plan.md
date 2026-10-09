# Plano do porte de `Worker` (fatia de medição, 2026-10-09)

Nada implementado. Estado no porte: `grep -i worker src/runtime` só acha menções incidentais
(`event_target.rs`, `post_message.rs`, `structured_clone.rs`); não há `Worker` global. O PLAN.md
lista `Worker` entre os globais "ainda ausentes" e não o descarta.

## Forma medida no bun 1.4.2

- `typeof Worker` é `function`, `length` 1, `name` `Worker`. Próprios da classe: `length`, `name`, `prototype`.
- Chamada sem `new`: `TypeError: Use \`new Worker(...)\` instead of \`Worker(...)\``.
- `new Worker()`: `TypeError: Not enough arguments`.
- `new Worker('')`, `new Worker('http://')` e `new Worker('nao-existe.js')` NÃO lançam no construtor
  (o erro de resolução chega depois, de forma assíncrona, como evento `error`). Reconferir o texto
  exato do evento `error` num golden próprio antes de implementar.
- `Object.getPrototypeOf(Worker.prototype) === EventTarget.prototype`.
- Nomes próprios de `Worker.prototype`, em ordem: `constructor`, `onerror`, `onmessage`,
  `onmessageerror`, `postMessage`, `ref`, `terminate`, `threadId`, `unref`, `getHeapSnapshot`,
  `getHeapStatistics`, `startCpuProfileInternal`, `stopCpuProfileInternal`, `cpuUsageInternal`.
- A instância não tem chave própria (`getOwnPropertyNames(w)` é `[]`); `onerror`/`onmessage`/
  `onmessageerror` são `null` no início; `onopen`/`onclose` são `undefined` (só existem
  `addEventListener('open'|'close')`). `threadId` é um inteiro crescente (1, 2, 3...).
- Eventos: `open` (antes da primeira mensagem), `message` (`MessageEvent`, `origin` e `lastEventId`
  vazios, `ports` vazio), `close` (evento de tipo `close`, depois de `terminate` ou da saída do worker).
- O inspect da instância lista os campos do protótipo como próprios (formato do `console.log`).

## O que o bun faz

1. Construtor: resolve o especificador (arquivo, `file:`, `data:`, `blob:`), valida opções
   (`name`, `type`, `env`, `argv`, `execArgv`, `workerData`, `ref`...), aloca `threadId` e sobe uma
   thread nativa nova com um VM/global próprio, isolado do pai.
2. `postMessage(value, transfer)` faz structured clone e enfileira para a thread do worker; a
   entrega é assíncrona, via laço de eventos do destinatário. No worker, `self.postMessage` /
   `self.onmessage` falam com o pai.
3. `onmessage`/`onerror`/`onmessageerror`: acessores no protótipo (atributo de evento) sobre o
   `EventTarget`. Exceção não tratada no worker vira evento `error` (`ErrorEvent`) no pai.
4. `terminate()` devolve promise, interrompe a thread e emite `close`.
5. `ref()`/`unref()` controlam se o worker segura o laço de eventos do pai (padrão: segura, como o
   `BroadcastChannel` aberto).
6. `getHeapSnapshot`, `getHeapStatistics`, `*CpuProfileInternal`, `cpuUsageInternal`: métodos de
   suporte ao `node:worker_threads`; medir retorno de cada um antes de portar.

## Reaproveitável no porte

- `src/runtime/event_target.rs`: `EventTarget`, `MessageEvent`, `ErrorEvent`, `CloseEvent`, classe
  `MessagePort` (sem construtor global ainda) e `global_has_listener`.
- `src/runtime/structured_clone.rs` e `post_message.rs`: clone e `add_post_message` do global.
- `src/runtime/broadcast_channel.rs`: modelo de fila entre contextos, `holds_event_loop`, ref/unref e
  o gancho no laço virtual (`timers.rs::run_event_loop`), que servem de molde.
- `uncaught_report.rs`: relato de exceção não tratada para o caminho do evento `error`.

## Falta e ordem sugerida

1. Golden de forma (`gen-worker-golden.js`): erros do construtor, nomes do protótipo, `threadId`,
   eventos `open`/`message`/`close`, `error` para URL inexistente.
2. Decisão de modelo: o porte roda um VM por thread? Confirmar no PLAN.md se o `JSGlobalObject`/heap
   é `Send`-isolável; senão, VM filho no mesmo thread com laço intercalado (observável via
   `threadId` e `isMainThread` apenas, sem diferença de formato).
3. Canal pai/filho sobre o par de filas do `BroadcastChannel` com `structured_clone` na borda.
4. Global do filho (`self`, `postMessage`, `onmessage`, `close`) e `workerData`/`parentPort`.
5. `terminate`/`ref`/`unref` e integração com `run_event_loop`.
6. Os cinco métodos de suporte por último.
