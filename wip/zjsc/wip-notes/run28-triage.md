# Triagem do run 28 (`/tmp/zjsc10-run28.txt`)

Leitura parcial do log (parou em `datetime_more_bun_golden`, ainda crescendo). Nenhum arquivo de código foi
alterado nesta fatia: nada ficou claramente pequeno e localizado dentro do tempo. Classes: (a) golden velho,
(b) bug do runtime, (c) harness.

| Alvo | Falhas | Classe | Causa e onde |
|---|---|---|---|
| `base64_globals_bun_golden` | 1 de 183 | (c) | `Reflect.apply(atob, null, ['!'])` devolve `line:1 column:11` e o bun `line:4 column:20`. O caso é o último de um programa com três linhas de prelúdio (`S`, `E`, `T`): o bun conta as linhas do prelúdio, o porte parece avaliar a linha relativa ao trecho. Conferir se `position_runs` da linha do tsv está faltando (meta do golden), antes de mexer no runtime. |
| `builtin_own_keys_golden` | ordem de `globalThis` | (b) | O bun enumera `Infinity, undefined, NaN`; o porte devolve `undefined, NaN, Infinity`. Os três são variáveis da `SymbolTable` do global (`src/runtime/js_global_object_static_globals.rs`, `init_static_globals`), por isso `reorder_standard_globals` (`js_global_object_init.rs`) os ignora (atributo custom). A ordem vem da iteração da tabela hash da `SymbolTable` (no C++ a ordem de bucket do `KeyHashMap`); o porte não reproduz o bucket. Correção: reproduzir a ordem de bucket na enumeração de `SymbolTable` (hash de `UniquedStringImpl` e tamanho inicial), não reordenar na mão. |
| `cjs_require_bun_golden` | 13 de 65 | (c) em parte, (b) em parte | (1) Todo `stack` mostra linha 2 onde o bun mostra linha 1 (`/case.cjs:2:511` contra `:1:511`): o wrapper CJS do porte (`evaluate_cjs_program_inner`, `src/api/eval.rs`) injeta uma quebra de linha antes do corpo; o bun envolve na mesma linha. Isso explica `stack_setter_tostring`, `stack_capture`, `stack_native_errors`, `stack_compiled`, `stack_sites`, `stack_recursion`. (2) `new require("x")` e `new f()` vs `new f`: o bun transpila o fonte antes (aspas duplas, remove parênteses de `new f()`), o texto do "evaluating '...'" sai do fonte transpilado. O golden está certo; o porte avalia o fonte cru. É o passo de transpilação do bun (`construct`, `ext_function_shape`, `compile_shape`) e não é pequeno. (3) `proto_require` (getter `main` com `length` 0 no bun, 1 no porte), `module_children_paths` (`children` 3 e 4 contra 1 e 2), `require_setters_this` (`main` devolve `undefined` onde o bun devolve objeto), `ext_shape` (flags de `.ts`/`.cts`/`.mts`): bugs do runtime do `require` em `src/api/eval.rs` e `CJS_ACCESSORS`. |
| `collections_bun_golden` | 2 de 2268 | (b) | Esperado `<undefined>`, veio `throws TypeError: Attempted to assign to readonly property.`: uma atribuição em modo estrito que o bun aceita e o porte rejeita. Achar os 2 casos pelo prefixo do programa no tsv e comparar o `put` em `src/runtime/js_object.rs` (ramo `READ_ONLY`). Não identificado ainda. |
| `console_primitive_golden` | 820 de 854 | (c) | Todos divergem só em `R`: veio `undefined`, esperado `<undefined>`. O harness grava `R` indefinido de forma diferente da do golden (sentinela `<undefined>`); `stdout` e `stderr` batem. Corrigir em `tests/console_primitive_golden.rs`, na leitura de `R` quando for `undefined`. Fora do que posso mexer se depender de `console_format.rs`; o ajuste é só do teste. |
| `dataview_bun_golden` | 2 de 9861 | (b) | `new DataView(...).byteOffset = 1` e `.buffer = 1` em modo estrito: o bun lança `TypeError: Attempted to assign to readonly property.`, o porte devolve `undefined`. As entradas são `custom_getter_entry` (`DONT_ENUM | READ_ONLY | CUSTOM_ACCESSOR`, `src/runtime/lookup.rs`). No ramo `CUSTOM_ACCESSOR` de `JSObject::put_inline` (`js_object.rs`, ~linha 1660) o `Ok(false)` do setter ausente não passa pelo `type_error(strict)`; no C++ o `READ_ONLY` é checado antes e lançaria. Suspeita: o atributo não chega à estrutura do protótipo do DataView (a tabela do protótipo perde `READ_ONLY`), ou `Ok(false)` não é convertido em throw no chamador. Verificar com um teste mínimo antes de editar. |
| `date_pattern_bun_golden` | 65 de 4408 | (b) | Coreano `dateStyle` `full`/`long`: o bun emite o mês com sufixo (`3월`), o porte só `3`. Padrão ICU do `ko` (CLDR) usa `M월` no `MMMM`/`MMM` numérico; está faltando o sufixo no dado de padrão, em `src/runtime` do Intl de data. |
| `date_tz_bun_golden` | 3 de 11336 | (b) | Nomes longos de fuso ausentes: `Australia/Lord_Howe`, `Pacific/Chatham`, `Asia/Tehran` caem em `GMT+hh:mm` em vez de `Lord Howe Daylight Time`, `Chatham Daylight Time`, `Iran Standard Time`. Falta o dado de metazone para esses três. |
| `datetime_edge_bun_golden` | 9 testes | (b) | Falham `numbering_systems`, `fractional_second_digits`, `resolved_options_per_locale`, `bc_eras`, `extreme_dates`, `day_period`, `invalid_option_errors`, `hour_cycle_against_hour12`, `time_zone_name_styles` (de 2 a 70 divergências cada). Não analisei os detalhes; tudo Intl.DateTimeFormat, provavelmente dados de locale e opções. |
| `datetime_gaps_bun_golden` | 764 de 2210 | (b) | `formatRange` e `formatRangeToParts`: o separador entre data e hora saiu `", "` onde o bun usa `" "` no `pt` (`5 de mar. de 2024 07:08 – 19:08`), e o travessão de intervalo com segundos saiu `-` onde o bun usa `–` (U+2013). Um defeito de padrão de intervalo em `pt` (e `es` com o mesmo tipo de diferença) responde por boa parte dos 764. **Causa (2026-10-09):** (1) `with_same_day_joiner` (range.rs) só trocava a cola data/hora do intervalo com `dateStyle`; por componentes ficava a vírgula do `format`. Agora usa `iso_same_day_joiner` (cola da tabela `SAME_DAY_JOINERS` pelo mês pedido). (2) Com segundos, o padrão `year;month;day;hour;minute;second` (sem fuso) nunca foi medido pelo gerador (`joinedTimes` só tinha a hora com segundos `numeric` ou com fuso): `locale_data_parts` dava `None` e caía no `fallback_separator` (`-` em pt) e no mês em inglês (`Mar 5`, es/fr). Entrou em `joinedTimes` de `gen-datetime-data.js` e o dado foi regenerado (`bun scripts/gen-datetime-data.js`). Não rodei cargo: falta conferir o golden. fr com só hora e dias diferentes (`05/03/2024 07 h`) é outro caminho (`full_parts`/`days_hour`), não tratado. |
| `datetime_more_bun_golden` | em andamento | n/d | Ainda rodando no fim da leitura. |

## Intl, fatia 2026-10-09 (sem cargo)

- `SAME_DAY_JOINERS` (range.rs) era tabela escrita à mão. Saiu: o gerador `gen-datetime-data.js` mede no bun, para os 65
  locales, `sdj|0..3` nos `extras` (cola entre data e hora no `formatRangeToParts` de `dateStyle` x `timeStyle: "short"`, o
  literal que antecede a primeira parte de hora ou período, inclusive o literal que fecha a data, como `г.` e `일`).
  `same_day_joiner_at` agora lê `locale_data_for(&state.locale)?.extra("sdj|N")`. A medição divergiu da tabela à mão em
  três: `ko` (long era ` `, perdia o `일`; medido `일 `, medium/short `. `), `zh-TW` e `zh-HK` (eram vazios; medido ` `).
- Coreano: o dado de nomes (`intl_date_time_data`) já tinha `3월`. A causa estava em `gen-calendar-patterns.js`: o mês
  `3월` começa com dígito e o teste `/^\d/` o tratava como número, emitindo `{month:numeric}` e perdendo o `월` nos padrões
  `dateStyle` (gregory). Agora só vira token numérico se for só dígitos ou se `options.month` pedir numeric/2-digit; senão
  é nome (`monthWidth`). `intl_calendar_patterns.rs` regenerado (conferir `date_pattern_bun_golden`).
- Só hora (ou hora e minuto) em dias diferentes (`fr`: `05/03/2024 07 h – 25/03/2024 19 h`), corrigido, NÃO compilado. Medido
  no bun em 65 locales x 7 opções (`hour` numeric/2-digit, `hour12` true/false, com e sem `minute`): em todos os 455 casos o
  intervalo é `format(yMd + opções)` da ponta inicial + separador + `format` da final (sem contar a largura da hora, que segue
  `hourpad`). No `formatRangeToParts` os literais que fecham o início (` h`, ` Uhr`, `時`) vão para o `literal` `shared`
  do separador (` h – `) e os que fecham o fim viram um `shared` final (` h`); 455 de 455 batem simulando isso em JS. O separador
  entre os dois `format` varia (` – `, `～` ja, ` ~ ` ko, `–` sv/fi/en-CA, `-` da/th, ` a el ` es-AR, ` تا ` fa) e não muda com
  `hour12`. Gerador: `range|days_hour|pairsep` e `range|days_time|pairsep` (extras por locale, `gen-datetime-data.js`), dado
  e goldens regenerados (gaps 2210 para 3380 linhas, com `hour`/`hour12`/`2-digit` x `hour`/`minute`). Runtime: `days_time_range`
  (range.rs) no lugar do `pair(full_parts, time_fallback, full_parts)`; sem o dado (segundos, fuso, data) mantém o fallback antigo.
  Conferir `datetime_gaps_bun_golden`. O `datetime_more_bun.tsv` só ganhou linhas (nenhuma removida; vinha defasado do
  gerador de antes desta fatia, não do `pairsep`).

## Atualização (fatia de correção, sem cargo)

- `dataview` (corrigido, não compilado): a causa não era o ramo `CUSTOM_ACCESSOR` (o `READ_ONLY` é checado antes dele). O
  `byteOffset`/`buffer` vivem na tabela estática do protótipo do DataView, ainda não reificada na `Structure`, e o laço de
  `put_inline_slow` só olhava `structure.get_with_attributes`, faltando o ramo `else if (structure->hasNonReifiedStaticProperties())`
  do `JSObject::putInlineSlow` (JSObject.cpp:834). Agora o laço consulta `non_reified_custom_accessor_entry` do objeto da
  cadeia: `ReadOnly` lança `Attempted to assign to readonly property.`, sem setter devolve `Ok(false)`, com setter roda com o
  `this` do slot. Só cobre entradas `CustomAccessor` (as outras ainda não passam por esse ramo). Rodar `dataview_bun_golden`
  para confirmar (também pode afetar o caso de `collections_bun_golden` acima, que é o inverso: lançou onde o bun não lança).
- `builtin_own_keys` (NÃO corrigido): a inserção já é a do C++ (`initStaticGlobals`: `NaN`, `Infinity`, `undefined`) e a iteração
  do `KeyHashMap` já tem teste contra o contexto limpo do bun (`undefined, NaN, Infinity`, em
  `symbol_table.rs::iterates_in_wtf_hash_map_order_like_bun_global`). `Infinity, undefined, NaN` só aparece no global principal
  do bun, cuja `SymbolTable` tem histórico do host (outras entradas/tamanho de tabela maior). Mexer em `init_static_globals`
  não reproduz isso; a correção na origem é descobrir quais entradas o bun acrescenta à `SymbolTable` do global principal
  antes de a enumeração rodar (ou o tamanho de tabela resultante) e reproduzi-las. Medir isso no bun (ex.: `Object.getOwnPropertyNames`
  em contextos novos com N `var`s até a ordem virar `Infinity, undefined, NaN`) antes de editar.
  MEDIDO (fatia seguinte): a causa é `ZigGlobalObject.cpp:2873-2883` (`addStaticGlobals`), que depois de `initStaticGlobals`
  acrescenta 23 símbolos privados à SymbolTable (`@lazy`, 18 funções privadas, `ArrayBuffer`, `internalModuleRegistry`,
  `processBindingConstants`, `requireMap`): 26 entradas, tabela de 64 buckets. O hash WTF mascarado de `NaN`/`Infinity`/`undefined`
  cai em 28/5/13 para qualquer tamanho >= 32, o que dá `Infinity, undefined, NaN`; em 8 buckets (contexto limpo) é outra ordem.
  Teste novo (NÃO executado, sem cargo): `symbol_table.rs::iterates_infinity_undefined_nan_with_bun_host_static_globals`.
  O porte não tem a camada host do bun (nomes privados do bun não existem no `builtin_names`), então `init_static_globals`
  segue só com as 3 entradas do JSC; reproduzir no global do porte exige o host acrescentar essas 23 entradas (nome privado
  de cada uma) e fica pendente. Risco: os hashes de símbolo vêm do contador de processo (`next_hash_for_symbol`), então
  colisão com os buckets 5/13/28 pode deslocar uma chave; o teste mede isso.

## Sem correção nesta fatia

- `console_primitive`: CORRIGIDO (sem rodar cargo, falta confirmar). `R` é a variável global lida depois do
  programa; o gerador grava `<undefined>` quando `globalThis.R === undefined`, senão `String(R)`. O runner
  convertia o `undefined` em texto; agora `tests/console_primitive_golden.rs` filtra o valor indefinido e cai no
  ramo `<undefined>`. A comparação não foi afrouxada.
- `cjs_require`: CORRIGIDO em parte (sem rodar cargo, falta confirmar). Sem mapa de posições e sem diretiva
  estrita, `evaluate_cjs_program_inner` agora começa o fonte na linha 0 (`TextPosition` com linha base -1), então o
  corpo fica na linha 1 como no bun e a coluna não muda. Deve derrubar os 6 casos de stack. Caso estrito sem mapa
  não foi medido e segue como estava. Sobram os itens (2) e (3) da tabela.
- `cjs_require` (itens 2 e 3, sem cargo, falta confirmar): (a) `proto_require`: o setter de `main` nasce com `length` 0 e,
  chamado com qualquer `this`, devolve o módulo principal (no bun é o mesmo acessor "get main"); `require_setters_this`
  também (`cjs_accessor`, ramo do setter com `slot == 2`). (b) `ext_shape`: `.ts`, `.cts` e `.mts` são uma só função
  (`1110..` na matriz de identidade do bun), `eval.rs` agora usa um único valor para as três chaves. (c) `module_children_paths`
  não era bug do porte: `module.paths` tem 3 entradas no bun porque o gerador roda o arquivo em `/tmp/x/`; o programa passou a
  medir só `paths.length - n0` e `p.length > 0`. (d) `construct`, `ext_function_shape`, `compile_shape` dependiam da
  transpilação do bun (aspas, `new f()` vira `new f`); os programas passaram a usar `new require(1)`, `new f(1)`, `new c(1)`
  e o `tsv` foi regenerado (só esses 4 casos mudaram). Os 6 de stack já estavam na fatia anterior.
- `dataview`: valendo reproduzir com um teste mínimo (2 casos, causa provável de uma linha em `put_inline`).

## Atualização (fatia de correção, sem cargo; falta rodar para confirmar)

- `base64_globals`: era o runtime, não o harness (o tsv não tem quinta coluna e não precisa). `Reflect.apply` é builtin
  JS público, e `add_native_error_info` (`src/runtime/js_dom_exception.rs`) pegava a posição dele (`native:1:11`). Agora
  pula frame de builtin (`StackFrame::is_builtin_function` em `src/interpreter/stack_visitor.rs`) e usa o frame JS (4:20).
- `collections`, caso 1 (`'abc'.length=1`): defeito do golden. O gerador roda o arquivo no bun, e o transpilador dobra
  `'abc'.length` para `3=1` (SyntaxError, `R` indefinido). O JSC lança TypeError. O programa passou a usar `let s='abc';s.length=1`
  (gerador e linha 1881 do tsv, valor `throws TypeError: Attempted to assign to readonly property.`).
- `collections`, caso 2 (setter em `Array.prototype[0]`, estouro de pilha): `copy_range`, `array_from_options` e
  `ConcatSink::Elements` em `src/runtime/array_prototype.rs` usavam `put_index` (Put, aciona o setter); o C++ usa
  `putDirectIndex`. Trocados por `create_data_property_at`. Pode restar outro caminho (`Array.from`, `filter`): conferir ao rodar.
