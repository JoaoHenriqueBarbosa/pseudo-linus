# Auditoria de funções de repasse em wip/zjsc (funções livres e construtores)

Fonte: `/tmp/new.out` (cópia de `scripts/dry-forwarders.py` sobre `wip/zjsc/src`). Os acessores
de campo (`self.campo.metodo()`) ficaram intactos. Nada foi compilado nem testado (cargo proibido
nesta rodada): a primeira compilação deve confirmar imports e visibilidade.

## Removidos (chamadores trocados pelo alvo direto)

| Repasse removido | Alvo | Observação |
|---|---|---|
| `{Eval,Program,ModuleProgram,Function}CodeBlock::create_copy_parsed_block` | `CodeBlock::create_copy_parsed_block` | `script_executable.rs` passa o ponteiro de função do alvo; import de `CodeBlock` acrescentado |
| `error_instance::materialize_stack_for_names` | `materialize_error_info` | passou a `pub`; chamador em `own_property_names.rs` |
| `JSArrayIterator::create_with_initial_values` | `JSArrayIterator::allocate` | só um teste chamava |
| `JSBigInt::try_create_zero` e `create_zero` | `JSBigInt::default()` | 11 chamadores em `js_big_int*.rs`; nota FATIA2 virou comentário |
| `TypeInfo::is_object_type` | `js_type::is_object_type` | `js_cell.rs`, `host_function_support.rs`; wrapper local de `js_typeof.rs` também saiu |
| `ArrayBuffer::create_from_contents` | `ArrayBuffer::new` | `new` passou a `pub` |
| `Math operations::round` | `math_common::js_round` | macro e testes de `math_object.rs` |
| `PropertyNameArray::create` | `PropertyNameArray::default()` | |
| `PropertyTable::new` | `PropertyTable::default` | 3 usos como ponteiro de função em `structure.rs` |
| `SparseArrayValueMap::new` | `SparseArrayValueMap::default()` | |
| `FastBitVector::new` | `FastBitVector::default()` | 4 arquivos |
| `StringView::new` | `StringView::default()` | 3 arquivos |
| `string_constructor::from_char_code` | `string_prototype::string_from_units` | |
| `wtf_string::null_string` | `String::default()` | 4 arquivos |
| `setup_llint` (privada, um chamador) | `llint_entrypoint::set_entrypoint` | inline, com a PENDÊNCIA no comentário |

## Removidos por não terem nenhum chamador

`JSArrayBuffer::to_wrapped_allow_resizable`, `JSOrderedHashMap::create_deleted_value` (e o import
de `Symbol`), `SmallStrings::empty_string` (o `impl` ficou vazio e saiu), `VM::defer_gc`.

## Mantidos, e por que não são repasse

- `glibc_math::d` (`f64::from_bits`): apelido de uma letra com cerca de 500 usos no porte do glibc,
  que espelha a macro do fonte C; trocar é mudança de escopo e não há como reexportar função associada.
- `StringView::span` (`T::view_span(*self)`): converte `&self` em cópia, método genérico sobre a trait.
- `native_error_constructor::is_native_error_type`: acrescenta a referência (`contains(&t)`), é predicado.
- `JSArray::try_create`: acrescenta argumento (`initial_length` duas vezes).
- `ScriptFetchParameters::integrity`: método de trait sem argumentos que devolve a string nula; não encaminha argumentos.
- `compute -> compute_impl` (`bytecode_basic_block.rs`) e os `emit_to_string`/`emit_type_of` (`emit_unary_op`): o
  primeiro é recursão/auxiliar com o mesmo nome do C++, os outros acrescentam o opcode; não foram tocados por serem métodos.
