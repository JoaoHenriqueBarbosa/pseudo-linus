# Avisos de terceiros

O `zjsc` embute código derivado do Bun (`oven-sh/bun`, tag `bun-v1.4.2`, commit
`744846f844374847c902b5e7fd59b4342a51ef99`), licenciado sob a MIT. O `LICENSE.md` desta pasta é o do upstream, copiado
sem alteração. O upstream não traz linha de copyright própria; o crédito é dos autores do Bun (`oven-sh/bun`).

| Arquivo | Origem no upstream |
|---|---|
| `src/api/bun_wrap.js` | `src/runtime.js`, copiado sem alteração (o módulo interno `bun:wrap`) |
| `src/api/node_events.js` | `src/js/node/events.ts`, portado para JavaScript comum |
| `src/api/node_async_hooks.js` | `src/js/node/async_hooks.ts`, portado para JavaScript comum |
| `src/api/node_timers.js` | `src/js/node/timers.ts`, portado para JavaScript comum |
| `src/api/node_timers_promises.js` | `src/js/node/timers.promises.ts`, portado para JavaScript comum |
| `src/api/node_querystring.js` | `src/js/node/querystring.ts`, portado para JavaScript comum |
| `src/api/node_stream_modules.js` | gerado por `scripts/gen-node-stream.js` a partir dos fontes do `stream` do upstream |
| `src/api/node_url.js` | gerado por `scripts/gen-node-url.js` a partir de `src/js/node/url.ts` e `src/js/internal/url.ts` |
| `src/api/node_util.js` | gerado por `scripts/gen-node-util.js` a partir de `src/js/node/util.ts` e dos internos que ele usa |
| `src/api/node_assert.js` | gerado por `scripts/gen-node-assert.js` a partir de `src/js/node/assert.ts` e dos internos que ele usa |
| `src/api/node_internal.js` | reescrita sobre o `internal/errors`, os validadores e o `internal/primordials` do upstream |
