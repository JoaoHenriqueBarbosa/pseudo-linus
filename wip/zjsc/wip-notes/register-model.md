# Modelo de registradores do gerador

Decisão: tudo que é `RegisterID*` ou `RefPtr<RegisterID>` no gerador é `RegisterRef` (anulável:
`Option<RegisterRef>`). `RegisterRef` incrementa a contagem ao nascer e decrementa ao soltar, como o `RefPtr`.

- Armazenamento (sem contagem, equivale ao `SegmentedVector<RegisterID>`): `callee_locals` na base,
  `parameters`, `constant_pool_registers`. Continuam `Rc<RefCell<RegisterID>>` (`RegisterIDRef`). Quem lê
  de lá embrulha com `RegisterRef::new(&storage[i])`, nunca `.clone()` do `Rc` direto num campo/argumento.
- Base: `new_register`, `new_temporary`, `add_var` devolvem `RegisterRef`; `new_temporaries` passa
  `&RegisterRef` ao closure.
- part3: todos os campos `Option<Rc<..>>` viraram `Option<RegisterRef>` (scope, thrown_value,
  scope_register, top_level_scope_register, arguments_register, lexical_environment_register,
  generator_register, empty_value_register, new_target_register, is_derived_constuctor,
  arrow_function_context_lexical_environment_register, promise_register, link_time_constant_registers).
  `generator_{state,value,resume_mode,frame}_register`, `register_for`, `kill`, `emit_load_completion_type`,
  `emit_load_resume_mode` usam `RegisterRef`.
- `IntoOperand<VirtualRegister>` para `RegisterRef`, `&RegisterRef`, `Option<RegisterRef>`,
  `&Option<RegisterRef>`, `Option<&RegisterRef>`; `IntoOperand<u8>` para `JSType`. As impls de
  `RegisterIDRef` continuam (o armazenamento ainda as usa).
- `kill` agora chama `static_property_analyzer.kill_register(&dst.borrow())` (o `kill()` do analisador
  não recebe registrador).

## Linhas dos fragmentos que ainda precisam mudar

- `bytecode_generator_cpp1.rs:630`: `Some(this.parameters[..].clone())` -> `Some(RegisterRef::new(&this.parameters[..]))`.
- `bytecode_generator_cpp2.rs:83`: idem (`parameters[..].clone()` num campo `Option<RegisterRef>`).
- `bytecode_generator_cpp2.rs:332`: `let source = self.parameters[i + 1].clone();` -> `RegisterRef::new(..)`.
- `bytecode_generator_cpp2.rs:942`, `:979`, `:993`: `constant_pool_registers[..].clone()` -> `RegisterRef::new(&..)`
  (`:979` devolve o registrador: o tipo de retorno vira `RegisterRef`).
- `bytecode_generator_part2.rs` comentários (linhas 2-3, 283-425) e `bytecode_generator_cpp1.rs:2`,
  `bytecode_generator_cpp4.rs:3` ainda descrevem `RegisterRef = Rc<RefCell<RegisterID>>` (só comentário).
- Qualquer chamada de `register_for(..)` que espere `Rc` (agora é `RegisterRef`) e de `kill(..)` com `&Rc`.
- `nodes_codegen_cpp2.rs:208`: o closure de `new_temporaries` recebe `&RegisterRef` (antes `&Rc`).
- Usos de `add_var()` (cpp1:694/728/988/1024/1041/1042, cpp1c:44/73, cpp2:89, cpp3:268/457/983, cpp5:464)
  agora recebem `RegisterRef`; `Some(this.add_var())` já compila nos campos novos, mas `cpp5:464`/`cpp3` que
  guardem em `Rc` precisam do tipo novo.
- `bytecode_generator_cpp6.rs` item 5 do errs-cpp6: `FinallyContext` em `bytecode_generator.rs` não tem
  campo de registrador `Rc` (usa `completion_record`); conferir `CompletionRecord`/`FinallyJump` por `Rc` de
  registrador se aparecerem erros.
