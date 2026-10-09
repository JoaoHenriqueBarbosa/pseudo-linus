# Varredura de regras, 2026-10-09

Escopo: arquivos alterados ou novos em `wip/zjsc` (sem `.tsv`, `.json`, `.lock`).

- Travessão (U+2014 e U+2013): nenhum fora dos dados do CLDR. A única ocorrência é a regex de
  escape em `scripts/gen-display-names-data.js`, que precisa citar os caracteres.
- Português sem acento: uma mensagem em `scripts/gen-e2e-golden.js` ("nao devolve numero"),
  corrigida. As demais ocorrências de `ja` e similares são códigos de locale.
- Identificadores em português em código novo: nenhum encontrado por grep de vocabulário comum.
- Assinatura ou menção de IA: nenhuma.
- Funções de repasse (`python3 -I scripts/dry-forwarders.py wip/zjsc/src`): ~400 achados nos
  arquivos alterados, quase todos acessores `self.campo.len`/`.clone`/`.push` do porte fiel do C++
  (espelham a API do JSC, `number_of_*`, `add_*`) e `new -> X::default`. Removido só
  `is_ws` em `src/wtf/date_math.rs` (virou `use ... as is_ws`).

Pendentes para decisão (não removidos, porque os chamadores são muitos ou espelham o C++):

- `create_copy_parsed_block` em eval/function/module_program/program_code_block -> `CodeBlock::...`
- `emit_check_traps`, `emit_super_sampler_begin/end`, `emit_unreachable` -> `OpX::emit`
- `call_frame.rs` `argument_offset`, `argument_offset_including_this`, `this_argument_offset`
- `js_big_int.rs` `try_create_zero`, `create_zero`, `create_with_length`, `try_create_from`
- `js_value.rs` `js_string`, `js_boolean`, `js_number_i32`, `js_number_u32`
- `operations_bitwise.rs` `js_lshift`, `js_rshift` -> `shift`
- `js_global_object_functions.rs` `parse_int_string`, `parse_int_number`
- `llint_entrypoint.rs` `default_call`, `arity_fixup` e dois entrypoints
- `vm.rs` `empty_string`, `defer_gc`; `math_object.rs` `round`; `string_constructor.rs` `from_char_code`
- `new -> X::default` em ~15 tipos (substituir por `Default` ou `#[derive]`)

# Varredura de funções de repasse em wip/zjsc (2026-10-09)

Método: `python3 -I scripts/dry-forwarders.py src/interpreter src/llint` e uma cópia temporária dos
arquivos `js_value*`, `js_object*`, `js_string*`, `structure*` de `src/runtime` (o script ignora o
diretório `wip` apenas como subdiretório, então a raiz passada funciona). 52 candidatos; a maioria
é acessor de campo (`self.x.get`, `self.cell.structure`, ...), que a regra isenta.

## Repasses removidos (10)

1. `CallFrame::argument_offset`, 2. `CallFrame::argument_offset_including_this`,
   3. `CallFrame::this_argument_offset` (call_frame.rs): só chamavam as funções livres de mesmo nome,
   sem nenhum chamador. Apagadas; as funções livres ficam.
4. `llint_entrypoint::default_call`, 5. `get_host_call_return_value_entrypoint`,
   6. `fuzzer_return_early_from_loop_hint_entrypoint`: só chamavam `LLIntEntry::X.code_ptr()`, sem
   chamador. Apagadas.
7. `js_value::js_string`: chamadores (js_template_object_descriptor, js_function_reify,
   bytecode_generator_cpp5) agora usam `JSValue::from_js_string` direto.
8. `js_value::js_boolean` virou `pub use JSValue::Bool as js_boolean;` (sem mexer nos ~70 arquivos).
9. `js_value::js_number_i32` virou `pub use JSValue::Int32 as js_number_i32;`.
10. `StructureCache::new` (só `default()`): o único chamador (js_global_object.rs) usa `default()`.

Nenhum `sed -i` em massa foi usado além de trocas pontuais em 3 arquivos de chamadores.

## Deixados de fora

- `js_number_u32 -> JSValue::from_u32`: associada, não reexportável, ~20 arquivos de chamadores com
  imports distintos; fica para quando houver compilação para validar.
- `ProtoCallFrame`, `Register`, `StackVisitor`, `CLoopStack`, `JSObject`, `JSString`, `Structure`:
  todos acessores de campo ou com conversão, isentos.
- `llint_entrypoint::arity_fixup` (`CodePtr::null()`): constante, não repasse de argumentos.

Nada foi compilado (regra da tarefa): conferir `cargo check` depois.

## Rodada 2: módulos novos de glibc e Intl (sem cargo)

1. Novo `src/runtime/glibc_words.rs`: `high_word`, `low_word`, `set_high_word`, `fma`. Tinham cópia em `glibc_hyper`
   (as quatro), `glibc_trig` (`high_word`, `low_word`) e `glibc_math` (`fma`). Os três passam a importar dele.
   `fma` continua uma função de uma linha (`mul_add`) porque ~67 chamadas usam a forma `fma(a, b, c)` do C; trocar
   por método sem compilar é arriscado. Candidato a remover quando houver `cargo check`.
2. `emulv`/`eadd`/`esub` já estavam num lugar só (`glibc_atan`, usados por `glibc_tan`): nada a fazer.
3. Novo `src/runtime/intl_table_lookup.rs` (`contains_sorted`, `sorted_index`, `sorted_value`, `linear_value`,
   `is_strictly_sorted`). Substitui a busca binária de `intl_date_time_data` (duas cópias), de
   `intl_collator_tailoring` (duas), `intl_locale_data::language_has_data` e o `find` linear de
   `intl_locale_getters_data`. O gerador `scripts/gen-datetime-data.js` emite o uso do helper.
4. Deixado de fora: `find` de `intl_display_names_data` (tabela de triplas, não pares), `MoreTable::lookup`
   (cadeia de pais, genérico próprio), buscas lineares de `intl_locale_data` sobre triplas, `intl_calendar_names::lookup`
   (chave de quatro campos). Parsers de tag: só `intl_locale_data::parse_language_tag` existe, sem cópia.
   `is_alpha` (locale_data, texto) e `is_alpha` (display_names, com tamanho) diferem de assinatura: candidato a unir.
   `wasm_ipint.rs`, `intl_number_*`, `intl_duration_format` e `temporal_calendar_icu.rs`: nenhuma duplicação
   evidente por leitura.

Nada foi compilado: conferir `cargo check` (tipos de `sorted_value` com `entries: &[(&str, u16)]`).
