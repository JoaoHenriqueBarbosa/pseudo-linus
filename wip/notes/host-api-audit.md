# Auditoria de APIs de host nos goldens

Regra: o porte é só o motor JSC, então um golden cujo resultado depende de API que o host do bun fornece não mede o motor. Isso vale também para `typeof X` e para a referência solta a `X` (no bun `typeof structuredClone` dá `function`, no porte dá `undefined`).

## O que foi feito

- 377 linhas removidas de 45 arquivos `tests/golden/*.tsv` (lista abaixo). Antes da edição, cópia dos TSV em `/tmp/golden-backup/` (os goldens não estão versionados no git).
- Remoção em massa feita com script Python temporário (`/tmp/rm_lines.py`, fora do repositório), porque são centenas de linhas em TSVs de até 25 MB. O script só apaga linhas inteiras, nunca reescreve conteúdo.
- Novo `scripts/host-api.js`, com `usesHostApi(programa)`, a lista única do que conta como API de host.
- 45 geradores em `scripts/` passaram a chamar o filtro antes de rodar o oráculo (uma linha de `require` e uma de `splice`/`delete` inserida por script de edição em massa, também temporário), de modo que uma regeneração não traz as linhas de volta. `node --check` passa em todos. `gen-options-parity.js` teve `"structuredClone"` retirado da lista com Edit.
- Verificação cruzada: `usesHostApi` aplicado às linhas do backup marca exatamente as linhas removidas (0 sobras nos TSVs atuais, 0 removidas sem marca). Não rodei os geradores nem o cargo.

## O que conta como host

Identificador solto (fora de propriedade) de: `structuredClone`, `queueMicrotask`, `setTimeout`, `atob`, `btoa`, `process`, `Bun`, `Buffer`, `fetch`, `TextEncoder`, `TextDecoder`, `URL`, `URLSearchParams`, `DOMException`, `ReadableStream`, `console`, `require`, `__dirname`, `__filename`, e as classes Web que `function_proto` sondava (`BroadcastChannel`, `CompressionStream`, `Crypto`, `CustomEvent`, `ErrorEvent`, `FormData`, `MessageEvent`, `PerformanceEntry`, `PerformanceObserver`, `PerformanceServerTiming`, `ReadableStreamDefaultController`, `ResolveError`, `TextDecoderStream`, `TransformStream`, `URLPattern`, `Worker`, `WritableStreamDefaultWriter`). Também `typeof global|module|exports`, `typeof self` (quando `self` não é nome local), `module.exports`, e em fixtures `.mjs` as propriedades de `import.meta` que o host preenche (`url`, `dir`, `file`, `path`, `dirname`, `filename`, `resolve`, `main`, `Object.keys(import.meta)` e afins).

Deixados de propósito (resultado igual em qualquer motor puro): `typeof window`, `typeof document`, `window.x` e `document.body` (ReferenceError do motor), `HTMLAllCollection`, `escape`/`unescape`, `import.meta` em `eval` fora de módulo (SyntaxError do motor), identidade `m === import.meta`, `delete import.meta`.

## Linhas removidas por arquivo

annexb 1, array 9, async 5, bigint 2, bigint_symbol 9, buffer 15, builtins 53, class 1, coercion_semantics 1, control_flow 7, ctor_this 1 (a linha 866 do exemplo), error_ctor 1, error_edge 1, error_message 1, errors 31, eval 1, eval_with 7, function_proto 20, global_edge 1, globals 1, iterator 1, json 4, json_deep 24, json_grid 1, limits 8, module 16, module_edge 28, module_more 32, object_model 10, options_parity 1, proxy_class 1, recent_apis 1, recent_features 1, reflect 1, sab 7, shadow_realm 22, shadow_realm_more 1, sloppy 1, stack_more 1, statements 1, symbol_weak 11, template_ops 16, typedarray_ctor 12, typedarray_proto 6, wasm_api 1, wasm_module 1.

Por arquivo, o motivo principal:

- Uso real de `structuredClone` (transfer de ArrayBuffer, clonagem de tipos, `=== undefined`): array, bigint, bigint_symbol, buffer, builtins, class, coercion_semantics, ctor_this, error_edge, errors, iterator, json_deep (24 linhas `structuredClone===undefined?0:...`), json_grid, limits, object_model, reflect, sab, shadow_realm, symbol_weak, typedarray_ctor, typedarray_proto, wasm_api, wasm_module, annexb, options_parity.
- Timers e microtarefas do host (`queueMicrotask`, `setTimeout`): async, control_flow, eval, eval_with, module, module_edge, shadow_realm, stack_more, statements, symbol_weak.
- Web APIs e Node: builtins (`atob`/`btoa`), errors, bigint_symbol, limits, function_proto (20 construtores Web), json (`Buffer`, `URL`, `URLSearchParams`, `TextEncoder`), proxy_class (`ReadableStream`), recent_apis, recent_features, shadow_realm_more, error_ctor (`DOMException`), shadow_realm (`typeof process|Bun|fetch|URL|TextEncoder|require|console|self`, `getOwnPropertyNames(globalThis).includes('Bun'|'console'|'process')`).
- `console`, `require`, `module`, `exports`, `self`, `global`: eval_with, globals, sloppy, global_edge, error_message, module, module_more, template_ops (`typeof console|process|require|module|exports|structuredClone|queueMicrotask|setTimeout`).
- `import.meta.*` do host: module, module_edge, module_more.

## Pisos dos testes

Todos os `total >= N` dos `tests/*_bun_golden.rs` afetados seguem satisfeitos. Os mais apertados: `module_edge` 600 de piso e 600 linhas (exato, zero folga); `recent_apis` 1700 de 1729; `global_edge` 500 de 549; `recent_features` 500 de 579; `coercion_semantics` 3000 de 3141; `reflect` 1400 de 1524. `options_bun_parity.rs` pede 60 expressões e o TSV tem 85. Se o `module_edge` ganhar nova exclusão, o piso precisa de caso novo ou o gerador de mais casos.

## Observações

- `template_ops_bun.tsv`, `scripts/gen-template-ops-golden.js` e `tests/template_ops_bun_golden.rs` são arquivos não versionados que outro trabalho estava escrevendo durante a auditoria (TSV de 25 MB modificado minutos antes). Removi as 16 linhas `typeof ...` de host dele e o gerador filtra por `p.body`; vale conferir que ninguém regenerou o TSV em paralelo. A lista `"typeof structuredClone", "typeof queueMicrotask", "typeof setTimeout", "typeof console", ... "typeof process", "typeof require", "typeof module", "typeof exports"` nas linhas 289 e 290 do gerador continua escrita lá, mas o filtro as descarta.
- `stack_format_bun.tsv` mudou 2 linhas durante a janela por outra mão (não foi esta auditoria).
- Arquivos `*_harness.js`/`*_prelude.js` e `async_bun_harness.js` usam `process`/`setTimeout`, mas só no lado do gerador/oráculo, não no programa do golden; não mexi.
- Uma remoção indevida minha (`module_more` linha 766, string `'self'` num `L('self')`) foi restaurada.
- Para regenerar um TSV específico com o filtro, basta rodar o gerador como antes; as linhas de host não aparecem mais.
