# Auditoria de segurança (unsafe, panics, recursão, alocação)

Data: 2026-10-08. Feita só por leitura e grep (sem compilar nem rodar testes).

## 1. unsafe

- `src/lib.rs:2` tem `#![forbid(unsafe_code)]` e `Cargo.toml:25` tem `unsafe_code = "forbid"`.
- O grep de `unsafe` em `src/` não achou nenhum bloco, função ou impl `unsafe` fora de comentário. Nada a remover ou justificar.
- Pendência: confirmar com `cargo check` (fora do meu alcance neste turno) que o `forbid` vale para todos os alvos.

## 2. Correção feita

- `VM::new` deixava `stack_limit` e `soft_stack_limit` em 0, então `is_safe_to_recurse()` nunca falhava e recursão JS profunda (interpretador, parser, `execute_call`, `execute_eval`) estouraria a pilha nativa e derrubaria o processo.
- Agora o limite duro nasce em (ponteiro de pilha atual) menos 1 MiB (`DEFAULT_STACK_BUDGET`), e o suave fica 64 KiB acima (`SOFT_STACK_MARGIN`). Arquivo: `src/runtime/vm.rs`.
- Risco: o `VM` pode ser criado numa thread com pilha menor que 1 MiB. Threads de teste do Rust têm 2 MiB, a main tem 8 MiB. Um embedder deve chamar `set_stack_limit` com o valor real se usar pilha menor.
- Pendência: rodar `cargo test` para checar que `yarr_matching_context_holder` (que lê `soft_stack_limit`) e os testes de recursão continuam passando.

## 3. Já protegido (lido, sem mudança)

- `JSON.stringify` e `JSON.parse` têm limites (`MAXIMUM_SIDE_STACK_RECURSION`, `MAXIMUM_RANGES_STACK_RECURSION`) e viram `StackOverflow`.
- Yarr (parser de padrão e interpretador) usa `StackCheck` com orçamento de 512 KiB.
- `String.prototype.pad` e `repeat` checam `MAX_LENGTH` e usam `checked_mul`.
- `new Array(n)` rejeita `n` que não seja uint32 (RangeError) e `try_create_with_hint` devolve `None` (OutOfMemory) acima de `MAX_STORAGE_VECTOR_LENGTH`.
- Os `Vec::with_capacity` em `array_prototype.rs` são limitados por `MAX_STORAGE_VECTOR_LENGTH`.

## 4. Pendências, não corrigidas

- Fechado em 2026-10-08 (sem compilar nem rodar testes, por leitura): alocação com tamanho vindo de JS não aborta mais o processo. O módulo novo `src/runtime/fallible_alloc.rs` (`try_vec_with_capacity`, `try_filled_vec`, `try_string_with_capacity`, `try_resize`, com teste) devolve `None` quando o alocador recusa, e cada chamador converte em `OutOfMemory` (`PutError`, `ArrayError::Put`, `Thrown`, `BigIntError`) ou no `nullptr` do C++.
- Vetores de array: `create_initial_{undecided,int32,double,contiguous}` (um só `create_initial_indexed_storage`), `create_initial_for_value_and_set`, `create_array_storage`, `convert_to_array_storage`, `ensure_array_storage_exists_and_enter_dictionary_indexing_mode`, `ensure_length_slow` (`try_resize`) e `ArrayStorage::try_new` (o `new` saiu; `new_base` e `empty` são de tamanho constante) devolvem `bool`/`Result` e os chamadores propagam `OutOfMemory` (`set_length`, `push`, `put_by_index*`, `put_direct_index*`). `try_create_uninitialized_restricted` devolve `None`. Acima de `MAX_STORAGE_VECTOR_LENGTH` já era `None`/RangeError, como no C++ (`tryCreate` devolve `nullptr`; o `ArrayStorage` esparso nunca materializa o vetor: `new Array(n)` com `ARRAY_WITH_ARRAY_STORAGE` usa `BASE_ARRAY_STORAGE_VECTOR_LEN`).
- `generic_arguments.rs`/`js_arguments_objects.rs`: `unmap_argument` e `set_modified_argument_descriptor` devolvem `Result<(), PutError>` e usam `try_filled_vec`. O comprimento nunca é o `arguments.length` reescrito pelo usuário (é `m_length`, o número de argumentos, limitado por `Interpreter::MAX_ARGUMENTS`), então o risco real era pequeno; agora não aborta.
- BigInt: o porte já tinha o limite do C++ (`MAX_LENGTH_BITS = 1 << 30`) em `**` (`exponentiate`), `<<`/`>>` (`to_shift_amount`, `result_length > MAX_LENGTH`), `asUintN`/`asIntN`, multiplicação (`result_length - 1 > MAX_LENGTH`), parse (`compute_length`) e `toString` (`MAX_LENGTH` de string). A mensagem é a do C++, "BigInt generated from this operation is too big" (`throwOutOfMemoryError`, um RangeError), não a do V8. Novo `try_zeroed_digits` nos buffers de resultado e rascunho de `**`, deslocamentos, `asUintN`, soma e subtração grandes, parse linear e `toString` (potência de dois, genérico e rápido). Testes em `js_big_int_ops.rs` fixam o TooBig antes de alocar.
- Outras alocações em `src/runtime` com tamanho de JS: `array_prototype.rs` (`values_with_capacity`, 9 sítios), `dense_snapshot`, `js_string_concat` (`None`), `JSCellButterfly::try_create`, `Uint8Array.fromBase64`.
- Resíduo, ainda `vec!`/`with_capacity` que pode abortar, mas de tamanho igual a um buffer já vivo ou constante (no C++ é `RELEASE_ASSERT`): `convert_undecided_to_double`, `switch_to_slow_put_array_storage` (o `haveABadTime` não tem como falhar), `js_global_object_functions.rs` (`encode`/`decode`, `characters.len()`), `string_prototype.rs:391`, `reg_exp_legacy_natives.rs:63`, `string_constructor.rs:48`, `js_object.rs:415` (`structure.inline_capacity()`), `json_object.rs:124` (gap, no máximo 10), `reg_exp.rs:314` (`ovector`), buffers internos de multiplicação e divisão de BigInt (`vec![0; 2 * k]` etc., limitados por operandos que já cabem em `MAX_LENGTH`). Falta compilar e rodar `cargo test` para tudo isto.

- `src/llint` e `src/interpreter`: `.expect(...)` em `slow_paths_control.rs` (linhas 98, 108, 138, 233, 409), `handlers_iterator.rs` (203-308), `handlers_array.rs:118`, `handlers_async.rs:62` são invariantes do bytecode (equivalentes a ASSERT do JSC), não entrada direta do usuário. Os de `handlers_iterator.rs` dependem de a verificação de fast path ter sido feita antes; vale um teste adversarial com `Array.prototype[Symbol.iterator]` trocado.
- `src/runtime` tem cerca de 966 ocorrências de unwrap/expect/panic/unreachable, `src/parser` 94, `src/yarr` 49: triagem linha a linha ainda não feita; não verifiquei indexação `[i]`. Prioridade: `string_prototype*`, `typed_array*`, `js_array_buffer`, `data_view` (índices vindos de JS).
- Recursão sem checagem de pilha identificada por leitura: `toString` de aninhamento profundo de arrays (`Array.prototype.join` recursivo) e `Object.prototype.toString`/`JSON.stringify` via `toJSON`: confirmar que passam por `is_safe_to_recurse` do `VM` (agora ativo).
