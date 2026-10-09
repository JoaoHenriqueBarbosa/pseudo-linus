# Checagem estática de integração (sem compilar)

Escopo: arquivos de `src/` modificados nas últimas horas (735 por mtime recente; o filtro `-newermt 2026-10-09` não pega nada porque o relógio da máquina está em 2026-10-08).

## Resultado: nenhuma correção foi necessária

1. Módulos: todo `mod` declarado em `src/runtime`, `src/wasm`, `src/yarr` e `src/llint` aponta arquivo existente. Os `.rs` sem `mod` (js_big_int_part2..9, yarr_interpreter_cpp1..6, yarr_pattern_cpp2..6, yarr_parser_part2) entram por `include!` nos hospedeiros (js_big_int.rs, yarr_interpreter.rs, yarr_pattern_cpp1.rs, yarr_parser.rs). Os diretórios `runtime/builtin_names` e `runtime/intl_date_time_format` estão declarados.
2. Imports: cada `use crate::...` de wasm_instance, wasm_memory, wasm_table, wasm_global, wasm_exception_type, wasm_ipint, js_web_assembly, icu_number, intl_duration_format, intl_list_format, intl_relative_time_format e intl_display_names acha a definição `pub` com o mesmo nome (`funcref_type` é `pub const fn`, `ListType`/`ListStyle`/`RoundingMode` vêm de `intl_enum!`, que emite `pub enum`). Aridade e tipos conferidos nas chamadas entre módulos (Table::try_create, Memory::try_create, Global::new, evaluate_extended_const_expr, ConstExprHost, format_parts, list_parts, cardinal_category, duration_sign, to_temporal_duration_record, construct_instance, with_instance, IntlClass::install, put_method_on, put_to_string_tag, install_wasm_errors, campos de ModuleInformation/TableInformation/GlobalInformation, variantes de ExceptionType e de TableInitializationType). As APIs externas de icu_decimal 2.3.0, icu_list 2.3.0 e fixed_decimal 0.7.2 usadas em icu_number.rs existem no registry local.
3. Duplicatas: sem colisão de nomes entre os arquivos incluídos por `include!` (js_big_int, yarr_interpreter, yarr_parser), nem entre slow_paths.rs e o `pub use slow_paths_arith::*`. `anychar_create` aparece em yarr_pattern.rs e yarr_pattern_cpp6.rs, mas são módulos distintos (yarr_pattern e yarr_pattern_cpp1), sem glob entre eles.

## Observação (sem efeito na compilação)

O comentário de cabeçalho de `yarr_pattern_cpp3.rs` diz que a fatia é incluída por `yarr_pattern.rs`; na verdade quem inclui é `yarr_pattern_cpp1.rs`.
