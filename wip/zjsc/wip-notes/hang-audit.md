# Auditoria de travamento: for-of no topo com `r += globalThis.Object.length`

Causa (por leitura, sem rodar): salto com alvo fora da faixa do operando. `set_label_location`
(bytecode_generator.rs) grava o deslocamento 0 no operando e guarda o alvo real em
`out_of_line_jump_targets` quando o salto para frente não cabe no tamanho do operando (narrow, 8 bits).
O laço de despacho (`dispatch_loop_from`, src/llint/dispatch.rs) tratava `Step::Jump(0)` como salto
para o próprio pc, então o `jfalse`/`jtrue`/`jmp` do for-of ficava re-executando para sempre. O corpo
com `globalThis.Object.length` (resolve_scope + get_from_scope + get_by_id extras) é o que empurra o
salto do fim do laço além da faixa; as variantes curtas cabiam e passavam.

Correção: no `Step::Jump`, deslocamento 0 consulta `UnlinkedCodeBlock::out_of_line_jump_offset(pc)`
(o `jumpTarget` do `.asm`, que lê o offset da tabela quando o operando é 0).

Pendente de confirmar rodando (não rodei nada): os dois scripts do relatório e a conferência de que
nenhum outro caminho de salto (switch, `op_jmp` de handlers_misc/dispatch_ext) devolve 0 sem passar
por `Step::Jump` (todos retornam `Step::Jump`, que é onde a correção está).

## Auditoria do restante do caminho de saltos longos (por leitura, sem rodar)

Conferido e sem defeito achado:
- `set_label_location` (bytecode_generator.rs) cobre os 23 opcodes de salto simples (jmp, jtrue, jfalse,
  jeq_null, jneq_null, jundefined_or_null, jnundefined_or_null, jeq, jstricteq, jneq, jeq_ptr, jneq_ptr,
  jnstricteq, jless..jngreatereq, jbelow, jbeloweq), a mesma lista do `SWITCH_JMP` de precise_jump_targets.rs.
  A chave da tabela é `instruction.offset()` (início da instrução, incluindo o prefixo wide), a mesma que o
  despacho usa (`pc`). Como no upstream, salto para frente nasce com `BoundLabel` 0 no tamanho narrow e só vai
  para a tabela quando não cabe; salto para trás com alvo conhecido escolhe wide16/wide32 no `fits_check`.
- Sentinela 0: é segura, salto real com deslocamento 0 (para o próprio pc) não existe (laço sempre tem
  `loop_hint` antes do `jmp` de volta); `add_out_of_line_jump_target` tem `assert!(target != 0)`. Nas tabelas de
  switch, `offset_for_value` já trata 0 como buraco e devolve o default (como o `.asm`), então o 0 de lá nunca chega
  ao `Step::Jump`.
- A tabela vive só no `UnlinkedCodeBlock` (`finalize` move do gerador); o `CodeBlock` consulta via
  `unlinked_code_block()`, sem cópia, então realms e execuções diferentes compartilham a mesma tabela.
- `BytecodeRewriter::adjust_jump_targets` (geradores, async, debugger) troca o mapa por um vazio e reinsere só os
  saltos que continuam fora da faixa, com `final_offset + instruction.offset()`; mesmo comportamento do upstream.
- Todos os desvios passam por `Step::Jump`: dispatch.rs (jmp, jtrue, jfalse), dispatch_ext.rs
  (`null_jump!`, `ptr_jump!`, switch_imm/char/string), handlers_misc.rs (`branch`, jmp). Nenhum faz `pc + offset`
  direto; o único `pc + offset` é o do próprio `Step::Jump` já corrigido.
- Handlers (`HandlerInfo`) guardam offsets absolutos de bytecode, `op_catch` não é salto; `loop_hint` e
  `check_traps` são `Step::Next`.
- Pequena ressalva: `out_of_line_jump_offset` só tem `debug_assert!`; em release, chave ausente faz o
  indexador entrar em pânico (não trava), o que é o comportamento desejado.

## Estouro de pilha nativa em `classic_loops_with_long_body` (por leitura, sem rodar)

Causa provável: `Drop` recursivo da lista de statements. Os `StatementNode` se encadeiam por
`next: Option<Statement>` (`Rc<RefCell<..>>`), então destruir 40000 `x += 1;` recursa 40000 vezes
(o C++ guarda um `Vector` em `SourceElements`). O emissor (`nodes_codegen_cpp4b.rs`, `statement =
current.base().next()`) e `SourceElements::iter` já são laços; não achei recursão por statement no parser.
Correção: `impl Drop for StatementNode` iterativo (src/parser/nodes.rs) e `Statement::strong_count`
no `define_node_enum!`. Não foi preciso mexer na thread do teste. Não executado; se ainda estourar,
checar liveness/CFG/BytecodeRewriter por recursão proporcional a blocos.

Teste novo: tests/long_jump_bytecode.rs (corpos de 200, 2000 e 40000 instruções em for-of, for, while,
do-while, for-in, if/else, ternário, switch imm e string, try/catch/finally, break/continue rotulados e
gerador). Não executado (sem cargo nesta tarefa): rodar `cargo test --test long_jump_bytecode`.
