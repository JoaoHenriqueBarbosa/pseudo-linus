# Auditoria dos runners de tests/*.rs contra afrouxamento

Data: 2026-10-09. Varredura estática (nada foi compilado nem rodado).

## Runners pedidos

`wasm_*_golden.rs`, `await_context_bun_golden.rs`, `async_gen_bun_golden.rs`, `date_parse_v8_bun_golden.rs`:
todos tratam `Ok(Err(_))` (script lançou) e pânico como FALHA, nunca como acerto. Nenhum tsv correspondente
espera lançamento de script (`SCRIPTERR`, `ERR`, `throw:` só aparecem como texto dentro de programas wasm_js, em
`catch` do próprio programa). A comparação do resultado é igualdade exata (`actual == expected`). Os `continue;`
nesses arquivos são filtros de faixa (`*_LINES`) ou de fuso, legítimos. Veredito: sem afrouxamento.

## Achados em tests/*.rs

| Arquivo | Padrão | Veredito |
|---|---|---|
| `statements_bun_golden.rs` | linha `TOP:` do golden só exigia que o programa lançasse, sem conferir nome e mensagem (229 linhas) | AFROUXAMENTO REAL, corrigido: usa `evaluate_named_script_reporting_uncaught`, o resultado vira `TOP:nome: mensagem` e entra na igualdade exata |
| `module_bun_golden.rs`, `module_edge_bun_golden.rs`, `module_more_bun_golden.rs` | erro esperado `BuildMessage:`/`ResolveMessage:`/`AggregateError:` aceitava qualquer erro, inclusive nenhum | AFROUXAMENTO, corrigido (2026-10-09): o golden já guarda `Nome: mensagem` normalizado (sem `/home` nem `/tmp`, 0 ocorrências; 65+14+207 linhas de BuildMessage/ResolveMessage/AggregateError), e o porte imita a camada (`src/runtime/js_module_loader.rs`). Os três runners agora comparam `actual_error == expected_error` exato, sem `is_bun_host_error`. Gerador intacto. Precisa de `cargo test --test module_bun_golden --test module_edge_bun_golden --test module_more_bun_golden`: divergência (p.ex. contagem do `AggregateError: N errors building`) é bug do porte, não motivo para afrouxar |
| `tailcall_bun_golden.rs` | pula os casos `sloppy_*` em que o bun devolve valor (11 casos); o contador não os registra | DIVERGÊNCIA CONHECIDA documentada (cauda sloppy vem do JIT no bun, o porte não tem JIT). É pulo real de caso: segue como dívida, não afrouxei. Falta contar os pulados no relatório |
| `subclass_edge_bun_golden.rs` | pula fontes com `String(Date(0))` | Legítimo: o esperado é o relógio da geração, não determinístico. Poderia conferir só o formato; baixa prioridade |
| `intl_edge/intl_collator/intl_more/intl_more_locales/reltime_more/datetime_edge` | `check(needle)` filtra linhas por `contains`/`starts_with` | Legítimo, e a cobertura foi conferida (2026-10-09, estática, cada fonte contra a lista de needles dos `check` do runner): linhas sem nenhum check = 0 em intl_edge (624), intl_collator (1840), intl_more (23498), intl_more_locales (1700), reltime_more (700) e datetime_edge (884, tags `/*tag*/` no início). Nenhuma linha fica sem teste; nenhum runner foi alterado. Se o gerador ganhar classe nova, repetir a conferência |
| `call_edge`, `gc`, `wasm_gc` etc. (`*_LINES`) | `continue` por faixa de env var | Legítimo, ferramenta de bissecção; o piso `total >=` é desligado só com a variável |
| `date_*`, `datetime_*` | `continue` por fuso | Legítimo (cada linha pertence a um fuso, todos os fusos rodam) |
| `unescape`/parsers (`regexp_exec`, `parser_syntax*`, `calendar`, `regexp_syntax`, `date_pattern`, `e2e_bytecode`) | `continue` dentro de laço de decodificação | Legítimo, não é filtro de caso |
| `math_bun_golden.rs`, `number_golden.rs` | `continue` em linha vazia | Legítimo |
| `calendar_bun_golden.rs:70-74` | filtra partes por prefixo no formato `narrow` | Legítimo (reproduz a regra do gerador); conferir se o gerador filtra igual |
| `error_stack_trace_limit.rs`, `jsonp_program.rs`, `global_readonly_names.rs`, `global_readonly_var.rs`, `global_with_symbol_table.rs` | `contains`/`starts_with` em mensagem ou pilha | Testes escritos à mão, sem golden. Frouxos para mensagem: `contains` aceita texto extra. Recomendo trocar por igualdade com o texto do bun quando houver oráculo (não alterei, exige medir no bun) |
| `heap_live_cells.rs` | `#[ignore]` com motivo (sem coletor) | Pulo honesto e documentado; segue como dívida do GC |
| `builtin_own_keys_golden.rs` | `bunOnly`/`skip` de chaves exclusivas do bun | Lista explícita de exceção, legítima desde que `bunOnly` não cresça sem motivo |

## Alterados nesta rodada

- `tests/statements_bun_golden.rs`
- `tests/module_bun_golden.rs`, `tests/module_edge_bun_golden.rs`, `tests/module_more_bun_golden.rs`

Nenhum cargo rodado: as alterações do `statements` precisam de um `cargo test --test statements_bun_golden` para
confirmar que as 229 linhas `TOP:` batem em nome e mensagem (se alguma divergir, é bug do porte a corrigir, não
motivo para voltar ao teste frouxo).
