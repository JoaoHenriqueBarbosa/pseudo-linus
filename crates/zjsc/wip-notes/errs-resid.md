# Erros residuais com causa em outro arquivo

- `bytecode_generator_cpp4.rs` `add_big_int_constant` (~788-793): `JSBigInt::parse_int` devolve
  `js_big_int::ImplResult` (enum `Empty | Heap | BigInt32`), mas o gerador (como o C++) trata o resultado
  como `JSValue` (`add_constant_value`, `big_int_map`). Falta `From<ImplResult> for JSValue` (ou o
  `ImplResult` virar `JSValue`, como o FATIA2 do próprio `js_big_int.rs` prevê).
- `bytecode_generator_cpp4.rs:1726`, `cpp5.rs:80`, `part2.rs:20/111/191`: `emit_bytecode` e
  `emit_bytecode_in_condition_context` em `Expression`/`Statement` dependem do despacho `impl
  Expression/Statement` (outro agente).
- `bytecode_generator_cpp5.rs` `emit_throw_*`: o erro medido dizia que
  `wtf_string::String::from_utf8(..)` virava `std::string::String` ao passar a `Identifier::from_string`
  (causa não encontrada na leitura); troquei por `Identifier::from_span(&vm, bytes)`.
