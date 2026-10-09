# Triagem de panic em src/yarr (PLAN.md item 10)

Critério: (a) invariante que o C++ garante com RELEASE_ASSERT/ASSERT, mantém; (b) caminho alcançável
por entrada do usuário (regex maliciosa, limite de pilha, OOM) em que o JSC lança erro, converte.

Resultado: **nenhum caso (b)**. Entrada do usuário (padrão, flags, profundidade de aninhamento,
tamanho) já passa pelo `ErrorCode` do parser ("RegExp too big", limite de parênteses aninhados,
SyntaxError) e pelo `JSRegExpResult::ErrorNoMemory`/`ErrorHitLimit` do interpretador. Nenhum dos
pontos abaixo é decidido por conteúdo do padrão.

## Código de teste (fora do escopo, `unwrap` em `#[cfg(test)]`)

- `yarr_flags.rs` 120, 135, 136, 150
- `yarr_unicode_properties.rs` 152, 154, 155, 158, 163
- `yarr_canonicalize.rs` 680

## (a) Invariantes, mantidos

| Arquivo:linha | Ponto | Asserção do upstream |
|---|---|---|
| `yarr_parser.rs` 460-472 | `CharacterClassParserDelegate::{assertion_word_boundary, atom_back_reference, atom_named_*}` | `RELEASE_ASSERT_NOT_REACHED()` em YarrParser.h; `parseEscape()` com `inCharacterClass` ligado só chama `atomPatternCharacter`/`atomBuiltInCharacterClass` |
| `yarr_parser.rs` 870-882 | mesmos quatro métodos no delegate de classe | idem |
| `yarr_parser.rs` 937-953 | mesmos quatro, mais `atom_built_in_character_class`, no delegate de disjunção de strings de classe (`/v`) | idem (`ClassStringDisjunctionParserDelegate`) |
| `yarr_parser.rs` 212, 216, 232, 263 | `active_capture_group_names`/`nested_capture_group_names` `.last_mut()` | `ASSERT(!m_activeCaptureGroupNames.isEmpty())`: o construtor empilha um elemento e `pop_parenthesis` só é chamado após `push_parenthesis` |
| `yarr_parser.rs` 242, 246, 252 | `pop_parenthesis` | `ASSERT(m_activeCaptureGroupNames.size() > 1)`; pareamento push/pop garantido pelo parser descendente recursivo |
| `yarr_parser_part2.rs` 1069 | `compile_mode(flags)` | `std::optional::operator*` sem conferência; chamadores sempre passam as flags |
| `yarr_pattern.rs` 622-672 | acessores `pattern_character`, `character_class`, `back_reference_subpattern_id`, `parentheses`, `anchors` | `ASSERT(m_type == ...)` nos acessores de `PatternTerm` (YarrPattern.h) |
| `yarr_pattern_cpp6.rs` 1706 | `atom_parentheses_end` sem pai | `ASSERT(parenthesisDisjunction->m_parent)`: disjunção de parênteses sempre criada por `atomParenthesesSubpatternBegin` com pai |
| `yarr_canonicalize.rs` 619 | `getCanonicalPair` fora dos tipos com par único | `RELEASE_ASSERT_NOT_REACHED()` em YarrCanonicalize.h; chamador confere `isCanonicallyUnique`/tipo antes |
| `yarr_interpreter_cpp3.rs` 181 | `backtrack_parentheses_terminal_end` | `RELEASE_ASSERT_NOT_REACHED()` em `Interpreter::backtrackParenthesesTerminalEnd` |
| `yarr_interpreter_cpp4.rs` 415, 422, 449, 562, 565 | `unreachable!()` no `match term.type_` do laço `matchDisjunction` (casos `SubpatternEnd` em backtrack, `BodyAlternativeEnd`, `DotStarEnclosure`, fim do `switch`) | `RELEASE_ASSERT_NOT_REACHED()` nos mesmos `case` e após o `switch` em `Interpreter::matchDisjunction` |
| `yarr_interpreter_cpp4.rs` 720, 726 | `body_disjunction()` / `_mut()` | `ASSERT(m_bodyDisjunction)` em `ByteCompiler`; sempre definido após `compile()` |
| `yarr_interpreter_cpp5.rs` 317 | `pop_parentheses_stack` vazio | `ASSERT(!m_parenthesesStack.isEmpty())` (já há `debug_assert!` acima) |

## (b) Convertidos

Nenhum. Nenhuma alteração de código foi necessária.

## Forma das mensagens (resolvido em 2026-10-09)

Os 13 `unreachable!()` de `yarr_parser.rs` (460-472, 870-882, 937-953) agora citam
`RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:LINHA (método)`, com as linhas 366-369 (classe de
`CharacterClassParserDelegate`), 771-774 e 840-844 conferidas no upstream. Os 5 de
`yarr_interpreter_cpp4.rs` (415, 422, 449, 562, 565) citam `YarrInterpreter.cpp` 2135, 2147, 2177,
2272 e 2275 (`matchDisjunction`). As edições foram feitas com `sed` por número de linha, porque as
linhas são idênticas e o `Edit` exigiria contexto único.

## Limites que NÃO são panic (conferidos)

Estouro de pilha e "RegExp too big" no parser e no byte compiler saem por `ErrorCode`
(`ParenthesesNestedTooDeep`, `PatternTooLarge`, `OffsetTooLarge`), não por panic.
