# Auditoria de fluxo de controle (golden contra o bun)

## O que foi criado

- `scripts/gen-control-flow-golden.js`: gera `tests/golden/control_flow_bun.tsv` rodando cada programa num processo
  bun 1.4.2 próprio (timeout de 10 s), com o harness de `tests/golden/async_bun_harness.js` (`L`, `tick`,
  `thenable`, ordem de microtarefas registrada no array `log`). Resultados com caminho da máquina são descartados.
- `tests/golden/control_flow_bun.tsv`: 1571 programas, cada linha com a fonte e o JSON do `log` depois de
  esvaziar as microtarefas, ou `error`, `name`, `message` (JSON) se o programa lançou de forma síncrona.
- `tests/control_flow_bun_golden.rs`: no padrão de `tests/async_bun_golden.rs` (realm novo por programa via
  `evaluate_named_script_result`, que esvazia as microtarefas; o mesmo harness embutido). Exige pelo menos 700
  programas. Ainda NÃO foi compilado nem rodado (a tarefa proibia cargo).

Regerar: `bun scripts/gen-control-flow-golden.js > tests/golden/control_flow_bun.tsv`.

## Cobertura do golden

- try/finally: 5 saídas do try x 5 saídas do finally x 5 tipos de laço (for, while, do-while, for-of, for-in), mais
  try/catch/finally em laço, finally aninhado, break/continue com rótulo através de finally, fechamento de
  iterador (`return()` do iterador) em break/throw/return dentro de for-of.
- switch (fallthrough, default no meio, comparação estrita, escopo léxico, TDZ), labels e erros de sintaxe de rótulo.
- destructuring (array/objeto, defaults, rest, iterador com `return`, parâmetros, for-of/for-in, catch).
- spread (array, chamada, new, super, objeto), tagged templates (cache do objeto de strings, raw, escapes inválidos).
- optional chaining e `??`/`??=` (curto-circuito, `this`, delete, privados), getters e setters (objeto, classe,
  privados, herança, Reflect), closures (let por iteração, TDZ, parâmetros com default, eval, with).
- iteradores e geradores (next/throw/return, `yield*` com delegação, protocolo de iterador, mutação durante a
  iteração), valor de completion de `eval`, mensagens de erro de runtime comuns.
- async/await, async geradores, for-await e ordem de microtarefas (thenables, `Promise.prototype.then` trocado,
  `await` de promessa com `constructor` alterado, `return` de promessa em async, filas de async gerador).

Um programa foi retirado do golden por laço infinito no bun (`for (let i = 0, inc = () => i++; i < 3; inc())`).

## Auditoria de `src/bytecompiler/nodes_codegen*.rs` (leitura, sem build)

Busca por `unimplemented!`, `todo!`, `Unported`, `TODO`, `panic!`, `not yet`, "ainda não portado":

- Nenhum `unimplemented!`, `todo!`, `Unported` ou `TODO` em `nodes_codegen*.rs` nem em `bytecode_generator*.rs`.
- try/finally: `TryNode::emit_bytecode` (`nodes_codegen_cpp5c.rs`, linhas 248 a 446) está completo: `FinallyContext`,
  `emit_out_of_line_finally_handler`, `emit_finally_completion`; break/continue/return passam por
  `emit_jump_via_finally_if_needed` e `emit_return_via_finally_if_needed` (`nodes_codegen_cpp5b.rs`, linhas 460, 503,
  554), com a guarda de tail call quando há finally (linha 534).
- for-await: `ForOfNode::emit_bytecode` (`nodes_codegen_cpp5b.rs`, linha 288) delega a `emit_enumeration`, que trata
  `is_for_await` em `bytecode_generator_cpp5.rs` (linhas 1498 a 1700, com `emit_get_generic_async_iterator` e
  `emit_async_iterator_next`). Sem ramo vazio ou stub.
- `panic!`/`unreachable!` restantes são os `RELEASE_ASSERT_NOT_REACHED` e `ASSERT(isXNode())` do C++ (casos que o
  parser não produz). Dois `panic!` em `nodes_codegen_cpp2.rs` (linhas 1352 e 1414) são `RELEASE_ASSERT(isBytecodeIntrinsicNode)`.

Pontos que dependem de código fora de `nodes_codegen*.rs` e que o golden vai exercitar (vistos nos cabeçalhos dos
arquivos como "dependências ainda não portadas, assumidas com o nome do C++"):

1. `DestructuringPatternNode::bind_value` / `bind_value_can_throw`, `AssignmentElementNode::emit_nodes_for_destructuring`
   e `ArrayPatternNode::bind_value` (cpp5b, cpp6, cpp7): conferir se existem hoje; todo o destructuring depende deles.
2. `ScopeNode::{using_declaration_count, has_await_using_declaration, emit_statements_bytecode}` (cpp5c): `using` e
   `await using` entram em `emit_body_with_using_if_needed`; fora do escopo deste golden, mas é caminho com try/finally implícito.
3. cpp5d (funções async/geradoras): lista dependências não portadas ao escrever (`ResumeMode`/`State` do
   gerador, `LinkTimeConstant::{NewResolvedPromise, NewRejectedPromise, AsyncFunctionDrive, ...}`,
   `emit_debug_hook`, `argument_offset(i32)` com tipo "por conferir"). Se algum ainda faltar, async/await falha
   na compilação do bytecode, não em tempo de execução.
4. `JSPropertyNameEnumerator::InitMode` é uma constante local (`CPP5B_ENUMERATOR_INIT_MODE = 0`) em `for-in`
   até a classe existir (`nodes_codegen_cpp5b.rs`, linha 7).
5. `nodes_codegen_cpp1b.rs:310`: FIXME herdado do C++ ("Use GetterSetter to store private accessors"), vale para
   getters/setters privados; é igual ao JSC, não é lacuna.

Conclusão: no nível do texto, não há caso de try/finally ou for-await sem tratamento em `nodes_codegen*.rs`. A
verificação real é rodar `cargo test --test control_flow_bun_golden` (pendente, por regra desta tarefa) e triar as
divergências por área.

## Auditoria dos cabeçalhos "dependências ainda não portadas" (2026-10-08)

Cada item foi conferido por grep em `src/`. Resultado: todos já existem; eram comentários desatualizados.
Nenhum stub, `Unported` ou retorno vazio encontrado, então nada precisou ser portado. Cabeçalhos corrigidos
em `nodes_codegen_cpp5b.rs`, `5c`, `5d`, `cpp6`, `cpp7`. Os `bytecode_generator_cpp*.rs` não têm esse tipo de lista.

- `DestructuringPatternNode::bind_value`/`bind_value_can_throw`: existem (cpp7, impl do enum).
- `ObjectPatternNode::bind_value`, `BindingNode::bind_value`/`bind_value_can_throw`: existem (cpp7).
- `ArrayPatternNode::bind_value`: existe (cpp6, linha 583).
- `AssignmentElementNode::emit_nodes_for_destructuring`/`bind_value_with_emitted_nodes`: existem (cpp7).
- `ScopeNode::emit_statements_bytecode`: existe (cpp5c, linha 494); `using_declaration_count`,
  `has_await_using_declaration`, `start_offset`, `line_start_offset`, `last_line` em `parser/nodes.rs`;
  `start_line`, `start_start_offset` em `parser/nodes_part2.rs`.
- `StringBuilder::append_quoted_json_string`: existe (`wtf/text/string_builder.rs:473`).
- `LinkTimeConstant::{NewResolvedPromise, NewRejectedPromise, Resolve/RejectPromiseWithFirstResolvingFunctionCallCheck,
  AsyncFunctionDrive}`: existem (`bytecode/bytecode_intrinsics_table.rs`).
- `js_generator::{Field, State, ResumeMode}`: existem (`runtime/js_generator.rs`; os módulos já são arquivo).
- `DebugHookType`: existe (`interpreter/interpreter.rs`); `argument_offset(i32) -> i32`: existe e o tipo confere
  (`interpreter/call_frame.rs:531`).
- Constante local de for-in: `JSPropertyNameEnumerator::InitMode` existe como `INIT_MODE: u8` em
  `runtime/js_property_name_enumerator.rs`; `CPP5B_ENUMERATOR_INIT_MODE` agora deriva dela
  (`INIT_MODE as u32`) em vez de repetir o 0. Mudança não compilada (sem cargo, por regra da tarefa).
