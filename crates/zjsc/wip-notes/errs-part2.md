# Pendências de bytecode_generator_part2.rs que dependem de outros arquivos

1. Despacho por família inexistente: `Statement::emit_bytecode(&self, generator, dst: Option<RegisterRef>)`,
   `Expression::emit_bytecode(&self, generator, dst) -> Option<RegisterRef>` e
   `Expression::emit_bytecode_in_condition_context(&self, generator, &Label, &Label, FallThroughMode)`.
   Chamados em part2 linhas 20, 111 e 190. Hoje só existem os corpos por struct concreta
   (`nodes_codegen_cpp*.rs`, o de condição recebe `this: &Expression` primeiro). Falta um `match` sobre as
   variantes (pode ser macro) em `nodes_codegen_cpp7.rs` ou arquivo novo, passando `this` onde o corpo pede.
2. `emit_bytecode_in_condition_context` por struct recebe `&Label` (não `&mut`); part2 foi ajustado.
3. `emit_is_*` em part2 agora recebem `src: &RegisterRef` (como `emit_is_cell_with_type` em cpp6); chamadores
   que passavam `Option<RegisterRef>` precisam passar `&RegisterRef`.
4. `JSTextPosition::is_valid` não existe; usei `is_set()` (o `operator bool` do C++).
