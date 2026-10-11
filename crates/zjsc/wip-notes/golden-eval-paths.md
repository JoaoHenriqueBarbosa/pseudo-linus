# Caminhos de avaliação dos goldens: gerador (bun) contra teste (porte)

Levantamento de leitura (283 `scripts/gen-*.js`, 2026-10-08). Classificação por análise estática do texto dos
geradores (script em `/tmp`, não versionado), com amostragem manual de cada família. A coluna de teste lista os
`tests/*.rs` que dão `include_str!` no tsv e as funções de `src/api/eval.rs` / `tests/common/mod.rs` citadas neles
(a lista é por arquivo, não por chamada: onde aparecem várias, o teste usa um subconjunto).

## Classes

- (a) arquivo gravado e executado como entrada do bun: vale a decisão ESM/CJS do bun (`moduleMode` em
  `scripts/golden-prelude.js`). 45 geradores.
- (b) `vm.runInThisContext`, `(0,eval)`, `new Function`, `vm.Script` ou filho que lê o programa do stdin e avalia:
  CJS não se aplica. 205 geradores.
- (c) outro: dados computados no gerador sem programa JS (Intl, locale, Math, número), módulos ESM por import
  (`runner` em module-*), sondas de reflexão. 33 geradores.

Detalhe da classe (a), 45 geradores:

| Subclasse | Geradores | Como o bun vê o arquivo |
|---|---|---|
| `canonicalSource` gravado | array, array-combo, array-edge, collections, iterator, reflect, string-unicode, tailcall | o texto canônico sempre começa com `"use strict"`, então o bun o executa como CJS estrito (wrapper), mesmo quando o original era ESM |
| prelúdio com `"use strict"` no topo | 16 (ex.: class, control-flow, error, function-*, limits, object-model, stack*) | CJS estrito |
| fonte crua, sem diretiva | 21: async, bytecode, control-flow-more, delete, generator-close, language-gap, microtask, promise, proxy, proxy-class, recent-apis, regexp, stack-column, string, wasm-* | ESM estrito se não há marcador CJS, CJS sloppy se há (`require`, `module`, `this` no topo, `with`...) |

`executableSource` não é usado por nenhum gerador (só definido em `golden-prelude.js`). `canonicalSource` é usado
por 8. Os 59 geradores com `--preload preload file` onde o arquivo é só `vm.runInThisContext(readFileSync(...))`
são (b), não (a): o arquivo de entrada é um embrulho vazio de semântica.

## Como o teste avalia

Todos os testes avaliam o programa como `Program` (script global) via `program_source` em `src/api/eval.rs`:
`evaluate_script` (54 tsvs), `evaluate_named_script_result` (a maioria, com o nome de arquivo e a global
`R`/`__final()` lida depois), `evaluate_script_sequence_result` (vários scripts), `evaluate_module_map` (módulos),
`evaluate_indirect_eval` (só 1 teste, o do `eval` indireto de verdade), com `run_golden`, `run_factored`,
`run_*_big_stack` de `tests/common/mod.rs` por cima. Não existe em nenhum teste um wrapper CJS
(`function(exports, require, module, __filename, __dirname)`), nem modo ESM de arquivo: `grep` por
`function(exports`, `__dirname`, `moduleMode`, `canonical` em `tests/` e `src/` não acha nada.

## Divergências apontadas

1. Classe (a) com `canonicalSource` ou prelúdio `"use strict"`: o bun roda como CJS estrito (wrapper), o teste roda
   como script estrito global. Diferem: `this` no topo (`module.exports` contra o global), `var`/`function` no topo
   viram variável local do wrapper no bun e propriedade global (não configurável) no teste, `arguments`/`return`
   no topo (válidos no wrapper, `SyntaxError`/`ReferenceError` no script), `require`/`module`/`exports` livres.
   Para CJS o `canonicalSource` devolve o corpo que o runtime põe no wrapper, e o comentário do `golden-prelude.js`
   diz que "o harness do porte avalia `function(exports, ...) {` + corpo + `}`", mas esse harness não existe nos
   testes. Divergência certa em 24 geradores.
2. Classe (a) com fonte crua e sem marcador: o bun roda como ESM estrito; o teste roda como script sloppy (sem
   diretiva). Estrito contra sloppy muda `this` em função solta, atribuição a não declarada, `arguments.callee`,
   `delete` de identificador, `with`. Os 21 geradores crus precisam de conferência individual: onde o programa cai
   num caso sloppy-sensível o tsv grava o erro estrito do ESM. Observação: vários desses (async, promise) passam
   o programa por um harness do tsv, não direto; conferir o que o harness do teste faz com o texto.
3. Classe (b) com `"use strict"` no prelúdio (`(0,eval)` no gerador, p. ex. json, class, scope): como é eval
   indireto, a diretiva só torna o eval estrito (CJS não entra, como previsto). Mas eval indireto estrito mantém
   `var`/`function` locais ao eval; o teste (script) as põe no global. Eval indireto sloppy cria `var` configurável
   (deletável) e `let`/`const` locais ao eval; script cria `var` não configurável e `let`/`const` globais
   visíveis a scripts seguintes. Só os testes que usam `evaluate_indirect_eval` reproduzem isto; os 205 demais
   usam script.
4. Classe (b) `vm.runInThisContext`: script sloppy, mesmo modelo do `Program` do teste. Sem divergência de modo;
   conferir só o nome do arquivo (`filename`) quando a pilha de erro entra no resultado, pois o gerador usa
   nomes como `annexb_case.js` e o teste passa o seu em `evaluate_named_script_result`.
5. Famílias com filho que lê o stdin e faz `(0, eval)(src)` (87 geradores): é o caminho de eval indireto, ver o
   item 3. Os testes dessas famílias usam `evaluate_script`/`evaluate_named_script_result`, não
   `evaluate_indirect_eval`.

## Tabela por gerador

Colunas: gerador (sem `gen-`), tsv, classe, caminho, uso de `canonicalSource`, testes (e funções citadas).
A classe `c` de `reflection` e `timezone-names` vem de sondas `bun -e` de reflexão sem programa de teste direto.

| Gerador | tsv | Classe | Caminho | canonicalSource | Teste [funções] |
|---|---|---|---|---|---|
| accessor-golden | accessor_bun.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador; usa Function() | - | accessor_bun_golden [evaluate_named_script_result,run_golden_big_stack] |
| annexb-golden | annexb_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | annexb_bun_golden [evaluate_script_sequence_result,run_golden] |
| annexb-methods-golden | annexb_methods_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | annexb_methods_bun_golden [evaluate_named_script_result,run_factored] |
| arguments-grid-golden | arguments_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | arguments_grid_bun_golden [evaluate_named_script_result,run_factored] |
| arguments-shape-golden | arguments_shape_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | arguments_shape_bun_golden [evaluate_named_script_result,run_factored_big_stack] |
| arguments-super-golden | arguments_super_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | arguments_super_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| array-combo-golden | array_combo_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | array_combo_bun_golden [evaluate_named_script_result,run_factored] |
| array-copy-golden | array_copy_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | array_copy_bun_golden [evaluate_named_script_result,run_factored] |
| array-edge-golden | array_edge_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | array_edge_bun_golden [evaluate_named_script_result] |
| array-exotic-golden | array_exotic_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | array_exotic_bun_golden [evaluate_named_script_result,run_factored] |
| array-generic-golden | array_generic_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | array_generic_bun_golden [evaluate_named_script_result,run_factored] |
| array-golden | array_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | array_bun_golden [evaluate_named_script_result,run_on_big_stack] |
| array-more-golden | array_more_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | array_more_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| async-gen-golden | async_gen_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | async_gen_bun_golden [evaluate_named_script_result] |
| async-golden | async_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | async_bun_golden [evaluate_named_script_result] |
| async-iter-grid-golden | async_iter_grid_bun.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador; usa Function() | - | async_iter_grid_bun_golden [evaluate_named_script_result] |
| async-order-golden | async_order_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | async_order_bun_golden [evaluate_named_script_result] |
| atomics-golden | atomics_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | atomics_bun_golden [evaluate_named_script_result,run_factored_big_stack] |
| available-locales | available_locales_bun.tsv | c | sem programa JS (dados computados no gerador) | - | intl_available_locales_bun_golden [evaluate_script] |
| await-context-golden | await_context_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | await_context_bun_golden [evaluate_named_script_result] |
| bigint-bun-golden | bigint_bun.tsv | b | `(0,eval)` no gerador | - | bigint_bun_golden [evaluate_named_script_result] |
| bigint-edge-golden | bigint_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | bigint_edge_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| bigint-golden | bigint.tsv | c | sem programa JS (dados computados no gerador) | - | bigint_golden [?] |
| bigint-grid-golden | bigint_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | bigint_grid_bun_golden [evaluate_named_script_result,run_factored] |
| bigint-symbol-golden | bigint_symbol_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | bigint_symbol_bun_golden [evaluate_named_script_result] |
| brand-check-golden | brand_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | brand_bun_golden [evaluate_named_script_result,run_golden] |
| buffer-edge-golden | buffer_edge_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | buffer_edge_bun_golden [evaluate_named_script_result,run_golden] |
| buffer-golden | buffer_bun.tsv | b | `(0,eval)` no gerador | - | buffer_bun_golden [evaluate_named_script_result,run_golden] |
| buffers-golden | buffers_bun.tsv | b | `(0,eval)` no gerador | - | buffers_bun_golden [evaluate_script] |
| builtin-descriptor-golden | builtin_descriptor_bun.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador | - | builtin_descriptor_bun_golden [evaluate_script_sequence_result] |
| builtin-iteration-golden | builtin_iteration_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | builtin_iteration_bun_golden [evaluate_named_script_result,run_factored] |
| builtin-shape-golden | builtin_shape_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | builtin_shape_bun_golden [evaluate_named_script_result,run_factored] |
| builtin-shape-intl-golden | builtin_shape_intl_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | builtin_shape_intl_bun_golden [evaluate_named_script_result,run_factored] |
| builtins-gap-golden | builtins_gap_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | builtins_gap_bun_golden [evaluate_named_script_result,run_golden] |
| builtins-golden | builtins_bun.tsv | b | `(0,eval)` no gerador | - | builtins_bun_golden [evaluate_script] |
| bytecode-golden | ? | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | (sem teste achado) |
| calendar-golden | calendar_bun.tsv | c | sem programa JS (dados computados no gerador) | - | calendar_bun_golden [evaluate_script] |
| calendar-patterns | ? | c | sem programa JS (dados computados no gerador) | - | (sem teste achado) |
| call-edge-golden | call_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | call_edge_bun_golden [evaluate_script_sequence_result] |
| case-golden | case_mapping.tsv | c | sem programa JS (dados computados no gerador) | - | case_mapping_golden [?] |
| class-builtin-golden | class_builtin_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | class_builtin_bun_golden [evaluate_named_script_result,run_factored] |
| class-edge-golden | class_edge_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito); usa Function() | - | class_edge_bun_golden [evaluate_named_script_result,run_golden] |
| class-golden | class_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | class_bun_golden [evaluate_named_script_result,run_factored] |
| class-grid-golden | class_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | class_grid_bun_golden [evaluate_named_script_result,run_factored] |
| class-order-golden | class_order_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | class_order_bun_golden [evaluate_named_script_result,run_factored] |
| coercion-golden | coercion_bun.tsv | b | `(0,eval)` no gerador | - | coercion_bun_golden [evaluate_named_script_result] |
| coercion-semantics-golden | coercion_semantics_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | coercion_semantics_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| collator-golden | collator_bun.tsv | b | `(0,eval)` no gerador | - | collator_bun_golden [evaluate_script] |
| collection-async-golden | collection_async_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | collection_async_bun_golden [evaluate_named_script_result] |
| collection-grid-golden | collection_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | collection_grid_bun_golden [evaluate_named_script_result,run_factored] |
| collection-mutation-golden | collection_mutation_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | collection_mutation_bun_golden [evaluate_script_sequence_result,run_golden] |
| collections-golden | collections_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | collections_bun_golden [evaluate_named_script_result,run_factored_big_stack] |
| completion-order-golden | completion_order_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | completion_order_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| completion-value-golden | completion_value_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | completion_value_bun_golden [evaluate_named_script_result] |
| completion-value-indirect-golden | completion_value_indirect_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | completion_value_indirect_bun_golden [evaluate_named_script_result] |
| control-effects-golden | control_effects_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | control_effects_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| control-flow-golden | control_flow_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | control_flow_bun_golden [evaluate_named_script_result] |
| control-flow-more-golden | control_flow_more_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | control_flow_more_bun_golden [evaluate_named_script_result] |
| ctor-this-golden | ctor_this_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | ctor_this_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| dataview-golden | dataview_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | dataview_bun_golden [evaluate_named_script_result,run_factored] |
| date-core-golden | date_core_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | date_core_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| date-edge-golden | date_edge_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_edge_bun_golden [evaluate_script] |
| date-golden | date_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_bun_golden [evaluate_script] |
| date-legacy-golden | date_legacy_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_legacy_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| date-legacy-parse-golden | date_legacy_parse_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_legacy_parse_bun_golden [evaluate_script] |
| date-parse-golden | date_parse_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_parse_bun_golden [evaluate_named_script_result,run_factored] |
| date-parse-v8-golden | date_parse_v8_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_parse_v8_bun_golden [evaluate_named_script_result] |
| date-pattern-golden | date_pattern_bun.tsv | c | sem programa JS (dados computados no gerador) | - | date_pattern_bun_golden [evaluate_script] |
| date-proto-golden | date_proto_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_proto_bun_golden [evaluate_script] |
| date-setters-golden | date_setters_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_setters_bun_golden [evaluate_named_script_result,run_factored] |
| date-tz-golden | date_tz_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | date_tz_bun_golden [evaluate_script] |
| date-utc-golden | date_utc_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | date_utc_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| datetime-data | datetime_more_bun.tsv | b | `(0,eval)` no gerador | - | datetime_more_bun_golden [evaluate_script] |
| datetime-edge-golden | datetime_edge_bun.tsv | b | `(0,eval)` no gerador | - | datetime_edge_bun_golden [evaluate_script] |
| datetime-range-golden | datetime_range_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | datetime_range_bun_golden [evaluate_named_script_result,run_factored] |
| define-own-property-golden | define_own_property_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | define_own_property_bun_golden [evaluate_named_script_result,run_factored] |
| define-property-grid-golden | define_property_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | define_property_grid_bun_golden [evaluate_named_script_result,run_factored] |
| delete-golden | delete_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | delete_bun_golden [evaluate_named_script_result,run_golden] |
| destructuring-golden | destructuring_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | destructuring_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| display-names-data | display_names_bun.tsv | c | sem programa JS (dados computados no gerador) | - | display_names_bun_golden [evaluate_script] |
| dispose-golden | dispose_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | dispose_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| duration-format-data | duration_format_bun.tsv | c | sem programa JS (dados computados no gerador) | - | duration_format_bun_golden [evaluate_script] |
| dynamic-fn-golden | dynamic_fn_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | dynamic_fn_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| e2e-golden | e2e_numeric.tsv | b | `(0,eval)` no gerador | - | e2e_numeric_golden [evaluate_script] |
| e2e-values-golden | e2e_values.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito); usa Function() | - | e2e_values_golden [evaluate_script] |
| enum-mutation-golden | enum_mutation_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | enum_mutation_bun_golden [evaluate_named_script_result,run_factored_big_stack] |
| error-api-golden | error_api_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | error_api_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| error-ctor-golden | error_ctor_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | error_ctor_bun_golden [evaluate_named_script_result,run_factored] |
| error-edge-golden | error_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | error_edge_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| error-golden | error_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | error_bun_golden [evaluate_named_script_result,run_golden] |
| error-message-golden | error_message_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito); usa Function() | - | error_message_bun_golden [evaluate_named_script_result,run_golden_big_stack] |
| error-stack-golden | error_stack_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | error_stack_bun_golden [evaluate_named_script_result,run_golden] |
| errors-golden | errors_bun.tsv | b | bun -e; usa Function() | - | errors_bun_golden [evaluate_script] |
| esnext-golden | esnext_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | esnext_bun_golden [evaluate_script_sequence_result] |
| eval-forin-golden | eval_forin_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | eval_forin_bun_golden [evaluate_named_script_result,run_factored] |
| eval-golden | eval_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | eval_bun_golden [evaluate_named_script_result,run_golden] |
| eval-scope-golden | eval_scope_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | eval_scope_bun_golden [evaluate_named_script_result,run_golden] |
| eval-with-golden | eval_with_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | eval_with_bun_golden [evaluate_named_script_result] |
| freeze-grid-golden | freeze_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | freeze_grid_bun_golden [evaluate_named_script_result,run_factored] |
| function-error-golden | function_error_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | function_error_bun_golden [evaluate_named_script_result,run_golden] |
| function-proto-golden | function_proto_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | function_proto_bun_golden [evaluate_named_script_result,run_golden] |
| function-scope-golden | function_scope_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | function_scope_bun_golden [evaluate_named_script_result,run_factored] |
| function-source-golden | function_source_bun.tsv | b | bun -e / arquivo misto; usa Function() | - | function_source_bun_golden [evaluate_named_script_result,run_golden] |
| function-tostring-golden | function_tostring_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | function_tostring_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| generator-close-golden | generator_close_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | generator_close_bun_golden [evaluate_named_script_result,run_factored_big_stack] |
| generator-golden | generator_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | generator_bun_golden [evaluate_named_script_result,run_golden] |
| generator-state-golden | generator_state_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | generator_state_bun_golden [evaluate_named_script_result,run_factored] |
| getter-setter-golden | accessor_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | accessor_bun_golden [evaluate_named_script_result,run_golden_big_stack] |
| global-edge-golden | global_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | global_edge_bun_golden [evaluate_script_sequence_result,run_golden] |
| global-semantics-golden | global_semantics_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | global_semantics_bun_golden [evaluate_script_sequence_result] |
| globals-golden | globals_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | globals_bun_golden [evaluate_named_script_reporting_uncaught] |
| ident-golden | ? | c | sem programa JS (dados computados no gerador); usa Function() | - | (sem teste achado) |
| intl-calendar-format-golden | intl_calendar_format_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | intl_calendar_format_bun_golden [evaluate_named_script_result,run_factored] |
| intl-collator-golden | intl_collator_bun.tsv | c | sem programa JS (dados computados no gerador); usa Function() | - | intl_collator_bun_golden [evaluate_script] |
| intl-edge-golden | intl_edge_bun.tsv | b | `(0,eval)` no gerador | - | intl_edge_bun_golden [evaluate_script] |
| intl-extra-golden | intl_extra_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | intl_extra_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| intl-golden | intl_bun.tsv | b | `(0,eval)` no gerador | - | intl_bun_golden [evaluate_script] |
| intl-list-plural-golden | intl_list_plural_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | intl_list_plural_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| intl-locale-golden | intl_locale_bun.tsv | b | `(0,eval)` no gerador | - | intl_locale_bun_golden [evaluate_script] |
| intl-misc-golden | intl_misc_bun.tsv | b | `(0,eval)` no gerador | - | intl_misc_bun_golden [evaluate_script] |
| intl-more-golden | intl_more_bun.tsv | c | sem programa JS (dados computados no gerador); usa Function() | - | intl_more_bun_golden [evaluate_script] |
| intl-more-locales-golden | intl_more_locales_bun.tsv | c | sem programa JS (dados computados no gerador); usa Function() | - | intl_more_locales_bun_golden [evaluate_script] |
| intl-object-golden | intl_object_bun.tsv | b | `(0,eval)` no gerador | - | intl_object_bun_golden [evaluate_script] |
| intl-text-golden | intl_text_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | intl_text_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| iter-protocol-golden | iter_protocol_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | iter_protocol_bun_golden [evaluate_indirect_eval,run_factored_big_stack] |
| iterator-golden | iterator_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | iterator_bun_golden [evaluate_named_script_result,run_factored] |
| iterator-helpers-golden | iterator_helpers_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | iterator_helpers_bun_golden [evaluate_named_script_result,run_golden] |
| iterator-protocol-golden | iterator_protocol_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | iterator_protocol_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| json-deep-golden | json_deep_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | json_deep_bun_golden [evaluate_named_script_result,run_factored] |
| json-golden | json_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | json_bun_golden [evaluate_named_script_result,run_golden] |
| json-grid-golden | json_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | json_grid_bun_golden [evaluate_named_script_result,run_factored] |
| json-more-golden | json_more_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | json_more_bun_golden [evaluate_named_script_result,run_golden] |
| json-number-golden | json_number_bun.tsv | b | `(0,eval)` no gerador | - | json_number_bun_golden [evaluate_script] |
| json-reviver-golden | json_reviver_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | json_reviver_bun_golden [evaluate_named_script_result,run_factored] |
| key-order-exotic-golden | key_order_exotic_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | key_order_exotic_bun_golden [evaluate_named_script_result,run_factored] |
| key-order-golden | key_order_bun.tsv | b | `(0,eval)` no gerador | - | key_order_bun_golden [evaluate_script] |
| language-gap-golden | language_gap_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | language_gap_bun_golden [evaluate_named_script_result,run_golden] |
| language-golden | language_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | language_bun_golden [evaluate_script] |
| lexer-grid-golden | lexer_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | lexer_grid_bun_golden [evaluate_named_script_result,run_factored] |
| limits-golden | limits_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | limits_bun_golden [evaluate_named_script_result,run_on_big_stack] |
| limits-range-golden | limits_range_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | limits_range_bun_golden [evaluate_named_script_result,run_on_big_stack] |
| list-format-patterns | ? | c | sem programa JS (dados computados no gerador) | - | (sem teste achado) |
| locale-aliases | ? | c | sem programa JS (dados computados no gerador) | - | (sem teste achado) |
| locale-bare-golden | locale_bare_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | locale_bare_bun_golden [evaluate_named_script_result,run_factored] |
| locale-data | locale_getters_bun.tsv | c | sem programa JS (dados computados no gerador) | - | locale_getters_bun_golden [evaluate_script] |
| locale-methods-golden | locale_methods_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | locale_methods_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| locale-more-golden | locale_more_bun.tsv | c | sem programa JS (dados computados no gerador) | - | locale_more_bun_golden [evaluate_script] |
| math-golden | math_bun.tsv | c | sem programa JS (dados computados no gerador) | - | math_bun_golden [?] |
| math-grid-golden | math_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | math_grid_bun_golden [evaluate_named_script_result,run_factored] |
| microtask-golden | microtask_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | microtask_bun_golden [evaluate_named_script_result] |
| microtask-order-golden | microtask_order_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | microtask_order_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| modern-syntax-golden | modern_syntax_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | modern_syntax_bun_golden [evaluate_named_script_result,run_golden] |
| module-edge-golden | module_edge_bun.tsv | c | runner de módulo (ESM por import); usa Function() | - | module_edge_bun_golden [evaluate_module_map] |
| module-golden | module_bun.tsv | c | runner de módulo (ESM por import) | - | module_bun_golden [evaluate_module_map] |
| module-more-golden | module_more_bun.tsv | c | runner de módulo (ESM por import); usa Function() | - | module_more_bun_golden [evaluate_module_map] |
| native-function-golden | native_function_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | native_function_bun_golden [evaluate_named_script_result,run_golden] |
| number-compact-golden | number_compact_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | number_compact_bun_golden [evaluate_named_script_result,run_golden] |
| number-convert-golden | number_convert_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | number_convert_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| number-edge-golden | number_edge_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | number_edge_bun_golden [evaluate_named_script_result,run_golden] |
| number-format-data | number_format_more_bun.tsv | b | `(0,eval)` no gerador | - | number_format_more_bun_golden [evaluate_script] |
| number-format-golden | number_format_bun.tsv | c | sem programa JS (dados computados no gerador) | - | number_format_bun_golden [evaluate_named_script_result,run_golden] |
| number-golden | number_to_string.tsv | c | sem programa JS (dados computados no gerador) | - | number_golden [?] |
| number-matrix-golden | number_matrix_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | number_matrix_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| number-parts-golden | number_parts_bun.tsv | b | `(0,eval)` no gerador | - | number_parts_bun_golden [evaluate_script] |
| number-proto-golden | number_proto_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | number_proto_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| number-range-golden | number_range_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | number_range_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| number-regional-golden | number_regional_bun.tsv | b | `(0,eval)` no gerador | - | number_regional_bun_golden [evaluate_script] |
| numeric-limits-golden | numeric_limits_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | numeric_limits_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| object-edge-golden | object_edge_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | object_edge_bun_golden [evaluate_named_script_result,run_golden] |
| object-literal-golden | object_literal_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | object_literal_bun_golden [evaluate_named_script_result] |
| object-model-golden | object_model_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | object_model_bun_golden [evaluate_named_script_result,run_factored] |
| object-proto-golden | object_proto_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | object_proto_bun_golden [evaluate_named_script_result,run_factored] |
| object-statics-golden | object_statics_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | object_statics_bun_golden [evaluate_script_sequence_result] |
| operator-edge-golden | operator_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | operator_edge_bun_golden [evaluate_script_sequence_result,run_golden] |
| operator-grid-golden | operator_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | operator_grid_bun_golden [evaluate_named_script_result] |
| options-parity | options_parity_bun.tsv | b | `(0,eval)` no gerador | - | options_bun_parity [evaluate_script,evaluate_named_script_result,evaluate_module_map,evaluate_script_matches_bun_options,evaluate_named_script_result_matches_bun_options,evaluate_module_map_matches_bun_options] |
| own-keys-golden | ? | c | sem programa JS (dados computados no gerador) | - | (sem teste achado) |
| parse-double-golden | parse_double.tsv | c | sem programa JS (dados computados no gerador) | - | parse_double_golden [?] |
| plural-golden | plural_bun.tsv | c | sem programa JS (dados computados no gerador) | - | plural_bun_golden [evaluate_script] |
| private-grid-golden | private_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | private_grid_bun_golden [evaluate_named_script_result,run_factored] |
| promise-golden | promise_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | promise_bun_golden [evaluate_named_script_result] |
| promise-grid-golden | promise_grid_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | promise_grid_bun_golden [evaluate_named_script_result] |
| promise-more-golden | promise_more_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | promise_more_bun_golden [evaluate_named_script_result] |
| proxy-chain-golden | proxy_chain_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | proxy_chain_bun_golden [evaluate_named_script_result,run_factored] |
| proxy-class-golden | proxy_class_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | proxy_class_bun_golden [evaluate_named_script_result,run_golden] |
| proxy-golden | proxy_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | proxy_bun_golden [evaluate_named_script_result,run_golden] |
| proxy-invariants-golden | proxy_invariants_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | proxy_invariants_bun_golden [evaluate_named_script_result] |
| proxy-reflect-golden | proxy_reflect_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | proxy_reflect_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| proxy-trace-golden | proxy_trace_bun.tsv | b | `(0,eval)` no gerador | - | proxy_trace_bun_golden [evaluate_named_script_result,run_golden] |
| queue-microtask-golden | queue_microtask_bun.tsv | b | `(0,eval)` no gerador | - | queue_microtask_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| recent-apis-golden | recent_apis_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | recent_apis_bun_golden [evaluate_named_script_result] |
| recent-features-golden | recent_features_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | recent_features_bun_golden [evaluate_named_script_result,run_golden] |
| reentrancy-golden | reentrancy_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | reentrancy_mutation [evaluate_script] |
| reflect-golden | reflect_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | reflect_bun_golden [evaluate_named_script_result,run_golden] |
| reflect-grid-golden | reflect_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | reflect_grid_bun_golden [evaluate_named_script_result] |
| reflection-golden | reflection_bun.tsv | c | bun -e | - | reflection_bun_golden [evaluate_script] |
| regexp-depth-golden | regexp_depth_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | regexp_depth_bun_golden [evaluate_named_script_result,run_factored] |
| regexp-edge-golden | regexp_edge_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | regexp_edge_bun_golden [evaluate_named_script_result,run_factored] |
| regexp-empty-match-golden | regexp_empty_match_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | regexp_empty_match_bun_golden [evaluate_named_script_result,run_factored] |
| regexp-exec-golden | regexp-exec.tsv | c | sem programa JS (dados computados no gerador) | - | regexp_exec_golden [?] |
| regexp-golden | regexp_v_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | regexp_v_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| regexp-legacy-golden | regexp_legacy_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | regexp_legacy_bun_golden [evaluate_named_script_result,run_golden] |
| regexp-modern-golden | regexp_modern_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | regexp_modern_bun_golden [evaluate_named_script_result] |
| regexp-more-golden | regexp_more_bun.tsv | b | `(0,eval)` no gerador; usa Function() | - | regexp_more_bun_golden [evaluate_script] |
| regexp-opt-golden | regexp_opt_bun.tsv | b | `(0,eval)` no gerador | - | regexp_opt_bun_golden [evaluate_script] |
| regexp-protocol-golden | regexp_protocol_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | regexp_protocol_bun_golden [evaluate_named_script_result] |
| regexp-receiver-golden | regexp_receiver_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | regexp_receiver_bun_golden [evaluate_named_script_result] |
| regexp-syntax-golden | regexp-syntax.tsv | c | sem programa JS (dados computados no gerador) | - | regexp_syntax_golden [?] |
| regexp-tables-golden | regexp_tables_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | regexp_tables_bun_golden [evaluate_named_script_result,run_golden] |
| regexp-v-golden | regexp_v_bun.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador; usa Function() | - | regexp_v_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| reltime-golden | reltime_bun.tsv | c | sem programa JS (dados computados no gerador) | - | reltime_bun_golden [evaluate_script] |
| reltime-more-golden | reltime_more_bun.tsv | b | `(0,eval)` no gerador | - | reltime_more_bun_golden [evaluate_script] |
| resolved-locale-golden | resolved_locale_bun.tsv | c | sem programa JS (dados computados no gerador) | - | intl_resolved_locale_bun_golden [evaluate_script] |
| sab-golden | sab_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | sab_bun_golden [evaluate_named_script_result,run_factored] |
| scope-golden | scope_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | scope_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| scope-grid-golden | scope_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | scope_grid_bun_golden [evaluate_named_script_result,run_factored] |
| segmenter-golden | segmenter_bun.tsv | c | sem programa JS (dados computados no gerador) | - | segmenter_bun_golden [evaluate_script] |
| segmenter-locales-golden | segmenter_locales_bun.tsv | c | sem programa JS (dados computados no gerador) | - | segmenter_locales_bun_golden [evaluate_script] |
| set-methods-golden | set_methods_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | set_methods_bun_golden [evaluate_named_script_result,run_factored] |
| setter-throw-golden | setter_throw_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | setter_throw_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| shadow-realm-golden | shadow_realm_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | shadow_realm_bun_golden [evaluate_named_script_result,run_golden] |
| shadow-realm-more-golden | shadow_realm_more_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | shadow_realm_more_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| shadow-realm-source-golden | shadow_realm_source_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | shadow_realm_source_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| sloppy-golden | sloppy_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | sloppy_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| sloppy-syntax-golden | sloppy_syntax_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | sloppy_syntax_bun_golden [evaluate_named_script_result,run_golden] |
| species-grid-golden | species_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | species_grid_bun_golden [evaluate_named_script_result,run_factored] |
| stack-column-golden | stack_column_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | stack_column_bun_golden [evaluate_script_sequence_result,run_golden] |
| stack-format-golden | stack_format_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | stack_format_bun_golden [evaluate_script_sequence_result,run_golden] |
| stack-golden | stack_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | stack_golden [evaluate_named_script_result] |
| stack-more-golden | stack_more_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | stack_more_bun_golden [evaluate_named_script_result] |
| stack-overflow-golden | stack_overflow_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | stack_overflow_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| statements-golden | statements_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | statements_bun_golden [evaluate_named_script_result] |
| string-coerce-golden | string_coerce_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | string_coerce_bun_golden [evaluate_named_script_result,run_factored] |
| string-golden | string_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | string_bun_golden [evaluate_named_script_result] |
| string-method-edge-golden | string_method_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | string_method_edge_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| string-receiver-args-golden | string_receiver_args_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | string_receiver_args_bun_golden [evaluate_named_script_result] |
| string-replace-golden | string_replace_bun.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador | - | string_replace_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| string-unicode-extra-golden | string_unicode_extra_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | string_unicode_extra_bun_golden [evaluate_named_script_result,run_factored] |
| string-unicode-golden | string_unicode_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | string_unicode_bun_golden [evaluate_named_script_result,run_golden] |
| string-unicode-more-golden | string_unicode_more_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | string_unicode_more_bun_golden [evaluate_named_script_result,run_golden] |
| subclass-edge-golden | subclass_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | subclass_edge_bun_golden [evaluate_script_sequence_result] |
| symbol-species-golden | symbol_species_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | symbol_species_bun_golden [evaluate_script_sequence_result,run_factored_big_stack] |
| symbol-weak-golden | symbol_weak_bun.tsv | a | prelúdio com "use strict" no topo: CJS estrito | - | symbol_weak_bun_golden [evaluate_named_script_result,run_factored] |
| syntax-early-golden | syntax_early_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | syntax_early_bun_golden [evaluate_named_script_result,run_golden] |
| syntax-error-positions-golden | syntax-error-positions.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador | - | parser_syntax_positions_golden [?] |
| syntax-errors-golden | syntax-errors.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador | - | parser_syntax_golden [?] |
| tailcall-golden | tailcall_bun.tsv | a | canonicalSource gravado como entrada (começa com "use strict": o bun o trata como CJS estrito) | canonicalSource | tailcall_bun_golden [evaluate_named_script_result] |
| tdz-grid-golden | tdz_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | tdz_grid_bun_golden [evaluate_named_script_result,run_factored] |
| template-edge-golden | template_edge_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa; usa Function() | - | template_edge_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| template-ops-golden | template_ops_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | template_ops_bun_golden [evaluate_named_script_result] |
| temporal-calendar-format-golden | temporal_calendar_format_bun.tsv | b | `(0,eval)` no gerador | - | temporal_calendar_format_bun_golden [evaluate_named_script_result,run_golden] |
| temporal-calendars-golden | temporal_calendars_bun.tsv | b | `(0,eval)` no gerador | - | temporal_calendars_bun_golden [evaluate_script] |
| temporal-duration-golden | temporal_duration_bun.tsv | b | `(0,eval)` no gerador | - | temporal_duration_bun_golden [evaluate_script] |
| temporal-edge-golden | temporal_edge_bun.tsv | b | `(0,eval)` no gerador | - | temporal_edge_bun_golden [evaluate_script] |
| temporal-golden | temporal_bun.tsv | b | `(0,eval)` no gerador | - | temporal_bun_golden [evaluate_script] |
| temporal-locale-golden | temporal_locale_bun.tsv | b | `(0,eval)` no gerador | - | temporal_locale_bun_golden [evaluate_script] |
| temporal-math-golden | temporal_math_bun.tsv | b | `vm.runInThisContext`/`vm.Script` no próprio gerador | - | temporal_math_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| temporal-plain-golden | temporal_plain_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | temporal_plain_bun_golden [evaluate_named_script_result,run_factored_big_stack] |
| temporal-round-golden | temporal_round_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | temporal_round_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| temporal-zoned-golden | temporal_zoned_bun.tsv | b | `(0,eval)` no gerador | - | temporal_zoned_bun_golden [evaluate_script] |
| text-locale-golden | text_locale_bun.tsv | b | `(0,eval)` no gerador | - | text_locale_bun_golden [evaluate_script] |
| this-binding-golden | this_binding_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | this_binding_bun_golden [evaluate_named_script_result,run_factored] |
| timers-golden | timers_bun.tsv | b | `(0,eval)` no gerador | - | timers_bun_golden [evaluate_script_sequence_result_running_timers,run_factored_big_stack] |
| timezone-golden | timezone_bun.tsv | b | bun -e | - | timezone_bun_golden [evaluate_named_script_result] |
| timezone-names | timezone_names_bun.tsv | c | bun -e | - | timezone_names_bun_golden [evaluate_script] |
| to-primitive-grid-golden | to_primitive_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | to_primitive_grid_bun_golden [evaluate_named_script_result,run_factored] |
| tostring-grid-golden | tostring_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)`; usa Function() | - | tostring_grid_bun_golden [evaluate_named_script_result,run_factored] |
| typedarray-ctor-golden | typedarray_ctor_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | typedarray_ctor_bun_golden [evaluate_named_script_result] |
| typedarray-edge-golden | typedarray_edge_bun.tsv | b | `(0,eval)` no gerador, programa com "use strict" (eval estrito) | - | typedarray_edge_bun_golden [evaluate_named_script_result,run_golden] |
| typedarray-golden | typedarray_bun.tsv | b | `(0,eval)` no gerador | - | typedarray_bun_golden [evaluate_script] |
| typedarray-more-golden | typedarray_more_bun.tsv | b | `(0,eval)` no gerador | - | typedarray_more_bun_golden [evaluate_script] |
| typedarray-proto-golden | typedarray_proto_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | typedarray_proto_bun_golden [evaluate_script_sequence_result] |
| unicode-grid-golden | unicode_grid_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | unicode_grid_bun_golden [evaluate_named_script_result,run_golden] |
| wasm-api-golden | wasm_api_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | wasm_api_bun_golden [evaluate_named_script_result] |
| wasm-exceptions-golden | wasm_exceptions_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | wasm_exceptions_bun_golden [evaluate_named_script_result] |
| wasm-funcref-golden | wasm_funcref_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | wasm_funcref_bun_golden [evaluate_named_script_result,run_factored] |
| wasm-gc-golden | wasm_gc_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | wasm_gc_bun_golden [evaluate_named_script_result] |
| wasm-js-golden | wasm_js_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | wasm_js_bun_golden [evaluate_named_script_result] |
| wasm-module-golden | wasm_module_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | wasm_module_bun_golden [evaluate_named_script_result,run_factored] |
| wasm-numeric-golden | wasm_numeric_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | wasm_numeric_bun_golden [evaluate_named_script_result] |
| wasm-simd-golden | wasm_simd_bun.tsv | a | fonte crua: ESM estrito ou CJS conforme marcadores (moduleMode) | - | wasm_simd_bun_golden [evaluate_named_script_result] |
| weak-more-golden | weak_more_bun.tsv | b | arquivo/runner chama `vm.runInThisContext` sobre o programa | - | weak_more_bun_golden [evaluate_script_sequence_result,run_golden_big_stack] |
| weak-symbol-golden | weak_symbol_bun.tsv | b | filho lê stdin, `(0,eval)(src)` | - | weak_symbol_bun_golden [evaluate_named_script_result,run_factored] |
