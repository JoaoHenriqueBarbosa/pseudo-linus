# Auditoria DRY (2026-10-08)

`scripts/dry-forwarders.py src` roda sobre `wip/zjsc/src` (é só leitura de texto, sem cargo): 870 repasses
no total, 406 nos arquivos alterados hoje. A maioria é acessor de campo (`self.x.len()`, `.clone()`),
que a regra não conta, ou nome espelhado do JSC.

## Os oito nomes citados

Nenhum é repasse puro (o script também não os acusa). Corpos com lógica própria:

- `create_tdz_error_from_source_range`, `create_invalid_private_name_error`, `throw_invalid_private_name`,
  `short_time_skeleton_state`, `same_day_joiner`, `to_wasm_value_owned` (escolhe entre duas funções),
  `string_object_own_slot`.
- `expression_info_for_bytecode_index` existe em dois tipos (`UnlinkedCodeBlock` e `CodeBlock`); o do
  `CodeBlock` soma `source_offset`, logo acrescenta conversão e não é repasse.

## Removidos

- `emit_pop_catch_scope` e `pop_class_head_lexical_scope` (repasse de `pop_lexical_scope_internal`): os dois
  chamadores (`nodes_codegen_cpp5c.rs`, `nodes_codegen_cpp6.rs`) chamam o alvo.
- `new_label_scope` (repasse de `new_label_scope_impl`): o `_impl` virou `new_label_scope`.
- `memory_to_fixed_length_buffer_body` (repasse de `memory_buffer_body`): o `host_function!` usa o alvo.
- `FunctionExecutable::baseline_code_block_for` e `profiled_code_block_for` (sem chamador fora da cadeia).
- `JSBigInt::try_create_from` (sem chamador; existe `try_create_from_impl`).
- `JSBigInt::create_with_length` (repasse de `create_with_length_impl`): o `_impl` virou `create_with_length`
  e os sete chamadores internos (`js_big_int.rs`, `js_big_int_part9.rs`) chamam o nome único.
- `tdz_error_for_text` em `exception_helpers.rs`: `create_tdz_error` e `create_tdz_error_from_source_range`
  passam a usá-la (a mensagem e o fallback ficam num lugar só). Sem cargo nesta rodada: conferir a compilação.
- `js_number_u32` (repasse de `JSValue::from_u32`): apagada, os 19 arquivos de chamadores (os de macro e os
  de caminho completo no `bytecompiler/`) chamam `JSValue::from_u32` direto. Sem cargo nesta rodada.
- `js_lshift`/`js_rshift` (repasse de `shift::<true>`/`shift::<false>`): apagadas; `shift` virou `pub` em
  `operations_bitwise.rs` e `slow_paths_arith.rs` e os testes chamam `shift::<_>` direto.
- `InstructionStreamWriter::new`, `GenericLabel::new` e `RegisterID::new` (só `Self::default()`): apagadas, os
  chamadores usam `::default()`. Sem cargo nesta rodada: conferir a compilação.

- `emit_check_traps`, `emit_super_sampler_begin`, `emit_super_sampler_end`, `emit_unreachable` (repasse de
  `OpX::emit`): apagadas, os quatro chamadores (`bytecode_generator_cpp1.rs`, `bytecode_generator_cpp2.rs`,
  `nodes_codegen_cpp2.rs`) chamam `OpX::emit(...)` direto.
- `uint64_to_double` e `uint32_to_float` em `wtf/dtoa/ieee.rs` (repasse de `f64::from_bits`/`f32::from_bits`):
  apagadas, os dois usos internos chamam o alvo.
- `null_string_view` em `string_view.rs` (repasse de `StringView::default`, sem chamador): apagada.
  Edição feita por script Python em lote (exceção à regra de Edit, por serem pares exatos em vários arquivos).
  Sem cargo nesta rodada: conferir a compilação.

- `new()` que só devolvia `Self::default()` em `BytecodeRewriter`, `StaticPropertyAnalyzer`, `Debugger`,
  `DebuggerPausePositions`, `SourceCode`, `UnlinkedSourceCode`, `VariableEnvironment`, `CodeCacheMap`, `CodeCache`,
  `OrderedTable`, `JsonRanges`, `URL`, `FixedVector`: apagadas, os chamadores (1 a 4 por tipo) usam `X::default()`.
  Edição em lote por script Python (pares exatos em vários arquivos). Sem cargo nesta rodada: conferir a compilação.

## Pendentes de `new() -> X::default()` (mantidos de propósito)

Com doc de divergência do C++ (a nota some se a função sair): `SourceCodeKey::new` (1 chamador, só teste; campos
`m_hash` e afins ficam em zero), `PropertyTable::new` (7 chamadores como `PropertyTable::new` em `structure.rs`;
a capacidade não existe aqui). Demais, por nome espelhado do C++ ou muitos chamadores: `ParserError::new` (10),
`StringView::new` (19), `FastBitVector::new` (8), `AtomString::new` (7), `SparseArrayValueMap::new` (5),
`PropertyNameArray::create` (11 com `null_identifier` e `try_create_zero`, três nomes distintos),
`Identifier::null_identifier`, `JSBigInt::try_create_zero`, `String::null_string` (API espelhada).

## Pendentes (candidatos, não mexidos por serem muitos chamadores ou API espelhada)

- `split_*` em `wtf_string.rs`: acrescentam o parâmetro const `ALLOW_EMPTY_ENTRIES` (e `split_with` etc. diferem
  só por ele), logo não são repasse puro pela regra; fundir `split_internal*` nos públicos com o const como
  parâmetro seria outra rodada.
- `compute -> compute_impl` em `bytecode_basic_block.rs`, `emit_to_string`/`emit_type_of -> emit_unary_op::<Op>`
  (o turbofish é conversão, não repasse), os quatro `create_copy_parsed_block`, `Vm::empty_string -> js_empty_string`.
