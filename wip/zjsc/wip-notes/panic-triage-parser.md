# Triagem de panic em src/parser (PLAN.md item 10)

Critério: (a) invariante que o C++ garante com RELEASE_ASSERT/ASSERT (ou desreferência sem conferência),
mantém; (b) caminho alcançável por fonte JS do usuário (fonte malformado, profundidade, limite de pilha)
em que o JSC dá SyntaxError ou RangeError, converte.

Método: `grep -nE '\.unwrap\(\)|\.expect\(|panic!|unreachable!' src/parser` (96 ocorrências fora de
comentário; o número 98 do pedido inclui duas linhas de comentário). Cada ponto foi lido no contexto e
conferido contra `upstream/JavaScriptCore/parser/`.

Resultado: **nenhum caso (b)**. Nenhum ponto é decidido por conteúdo do fonte JS. Fonte malformado sai
por `fail_if_*`/`semantic_fail_*` (SyntaxError com a mensagem do upstream) e a profundidade sai por
`fail_if_stack_overflow!` (`parser_cpp2.rs` 84, 575, 682, 723; `parser_cpp6.rs` 544, 670;
`parser_cpp8.rs` 76), que vira `ErrorType::StackOverflow` em `parser_part3.rs` 650, 743, o mesmo
caminho de `failIfStackOverflow()` no C++ (o chamador converte em RangeError). Por isso nenhum teste
novo e nenhuma alteração de código foram necessários.

## Código de teste (fora do escopo, dentro de `#[cfg(test)]`)

- `position_map.rs` 79, 86, 95, 97
- `variable_environment.rs` 897

## (a) Invariantes, mantidos

| Arquivo:linha | Ponto | Asserção do upstream |
|---|---|---|
| `parser_cpp6.rs` 590 | `meta_property_name` sem new.target/import.meta | `RELEASE_ASSERT_NOT_REACHED()` em Parser.cpp 4290; só chamada com `isMetaProperty(lhs)` (Parser.cpp 4455) |
| `parser_cpp8.rs` 69 | nome de operador unário desconhecido | mesmo `switch` sobre os tokens que o laço unário aceita |
| `parser_cpp8.rs` 811 | `default` do `switch` de operadores unários | `CRASH()` em Parser.cpp 5880 ("something has gone horribly wrong") |
| `parser_cpp3.rs` 194 | `declaration_type` sem token de declaração | `RELEASE_ASSERT_NOT_REACHED()`; o chamador só entra com var/let/const/using/await using |
| `parser_cpp4.rs` 92, 115 | prefixo "a"/"an" para modos Program/Module/ClassField/StaticBlock | `RELEASE_ASSERT_NOT_REACHED()` (Parser.cpp 1599/2095 região); esses modos não passam por mensagem de função |
| `parser_part2.rs` 454 | destructuring de `using` | `RELEASE_ASSERT_NOT_REACHED()`; rejeitado antes na gramática de `using` |
| `parser_part3.rs` 445, 455 | `disallowed_identifier_await_reason` / `_yield_reason` | `RELEASE_ASSERT_NOT_REACHED()` ao fim das duas funções; só chamadas depois de `isDisallowedIdentifier*` |
| `syntax_checker.rs` 216 | `get_metadata` | `static NO_RETURN_DUE_TO_CRASH ... getMetadata(ParserFunctionInfo<SyntaxChecker>&) { RELEASE_ASSERT_NOT_REACHED(); }` (Parser.cpp 2943); só roda com `CREATES_AST` |
| `ast_builder.rs` 193, 196, 202 | `end_offset`/`set_end_offset`/`breakpoint_location` de `CaseClauseNode` | `RELEASE_ASSERT_NOT_REACHED()` nas outras sobrecargas de `CaseClauseNode` |
| `ast_builder.rs` 1015 | `create_class_decl_statement` sem `ClassExpr` | `RELEASE_ASSERT_NOT_REACHED()`; o parser só passa `ClassExprNode` |
| `ast_builder_part2.rs` 51 | `append_to_comma_expr` | `ASSERT(tail->isCommaNode())` (ASTBuilder.h 901) |
| `ast_builder_part4.rs` 53, 196, 271 | acessores de ponto | `ASSERT(expr/func/loc->isDotAccessorNode())` (ASTBuilder.h 1261, 1510, 1677) |
| `ast_builder_part3.rs` 16, 24, 33, 71 | `static_cast` para Array/ObjectPattern, NumberNode, BigIntNode | `static_cast` do C++ após `isNumber()`/`isArrayPattern()` etc. testados pelo chamador |
| `ast_builder_part3.rs` 282 | operador binário desconhecido | `CRASH()` no `default` de `makeBinaryNode`; só chega token da tabela de precedência |
| `ast_builder.rs` 113, 141, 146, 403, 408, 529, 926 | alça nula, nó nulo, nome nulo, `FunctionMetadataNode` nulo | o C++ desreferencia o ponteiro sem conferir (`ASSERT(functionInfo.name)` em Parser.cpp 2776/2868/2989; nome de propriedade nunca nulo em `createProperty`) |
| `parser_cpp5.rs` 133, 216 | `function_info.name` | `ASSERT(functionInfo.name)` (Parser.cpp 2776, 2868, 2989) |
| `parser_cpp5.rs` 272 | `info.class_name` | `ASSERT(info.className)` (Parser.cpp 3106, 3168) |
| `parser_cpp5.rs` 331, 425, 452, 474, 510, 519, 983 | `token.data.ident` / `big_int_string` | `ASSERT(ident)` (Parser.cpp 3237, 3266, 3277, 3291, 4600, 5673); o lexer preenche `ident` em IDENT, STRING, PRIVATENAME e palavras-chave com as flags padrão, e `bigIntString` em BIGINT (Lexer.cpp 2586, 2625, 2665, 2755; `lexer_part4.rs` 671, 850). As flags `DontBuildStrings`/`DontBuildKeywords` só existem no SyntaxChecker, que não passa por estes pontos de classe/módulo |
| `parser_cpp5.rs` 954, 955, 963, 964, 965 | `pop()` das pilhas de `if`/`else` | `takeLast()` com pilhas empilhadas em pares no mesmo laço |
| `parser_cpp6.rs` 725, 739 | `save_point` | criado exatamente quando `maybeValidArrowFunctionStart`/`maybeAssignmentPattern` |
| `parser_part2.rs` 434 | `pop_call_or_apply_depth_scope` vazio | destrutor de `CallOrApplyDepthScope`, sempre pareado com o construtor |
| `parser_part2.rs` 499 | `current_scope()` nulo | `m_currentScope` sempre definido durante o parse |
| `parser_part2.rs` 505, 513, 651, 730, 753, 775, 803; `parser_part3.rs` 286, 297, 321 | `containing_scope()` | laços de Parser.h 1277-1301: só sobem enquanto `scope->containingScope()` existe (já comentado) |
| `parser_part2.rs` 711 | `cleanup_scope.scope()` | `AutoCleanupLexicalScope`, escopo empurrado antes |
| `parser_part3.rs` 682 | `before` de `reportParseTimes` | `start_parse_timer` devolve `Some` sempre que `Options::report_parse_times()` está ligada |
| `parser_part3.rs` 775, 863, 934; `lexer.rs` 889, 1028, 1029; `lexer_part2.rs` 34; `lexer.rs` 1324; `unlinked_source_code.rs` 36; `parser_arena.rs` 219 | `SourceCode` sem provider, lexer usado antes de `set_code`, arena ausente | o C++ desreferencia `provider()`/`m_source` sem conferir; os chamadores constroem o `SourceCode` com provider e chamam `setCode` antes de qualquer token. Entrada JS não influencia |
| `parser.rs` 248; `variable_environment.rs` 278, 289, 411, 437, 442, 498, 503, 513, 518, 574 | `Identifier::impl_()` nulo | o C++ desreferencia o `StringImpl*`; identificadores do parser nunca são nulos (nome nulo só é usado como sentinela e nunca chega a estes métodos) |
| `variable_environment.rs` 324, 365, 371 | variável a capturar/importar/exportar fora do ambiente | `RELEASE_ASSERT` em `VariableEnvironment::markVariableAsCaptured/Imported/Exported`, já com mensagem do upstream |
| `variable_environment.rs` 680, 694 | ambiente compacto/inflado | `ASSERT` em `toTDZEnvironmentSlow`/`Variables::Compact`: troca só ocorre uma vez |
| `variable_environment.rs` 821, 831, 848 | `Handle` sem ambiente ou ausente do mapa | `RELEASE_ASSERT` em `VariableEnvironment::Handle` |
| `nodes_part3.rs` 173 | `ModuleProgramNode` sem `ModuleScopeData` | `ASSERT` no construtor: só criado em modo módulo, onde o parser fornece os dados |

## (b) Convertidos

Nenhum.

## Limites que NÃO são panic (conferidos)

- Profundidade de aninhamento (expressões, statements, padrões, funções): `fail_if_stack_overflow!`
  vira `ErrorType::StackOverflow` e o chamador converte em RangeError (`parser_part3.rs` 650, 743).
- Fonte malformado, palavras reservadas, `await`/`yield` fora de contexto, `new.target = 1`,
  `delete` de identificador em modo estrito, nome de BigInt inválido: todos por `fail_if_*` /
  `semantic_fail_*` com a mensagem do upstream.

## Forma das mensagens (resolvido em 2026-10-09)

`parser.rs` 248 e os 10 `impl_().expect` de `variable_environment.rs` (278, 289, 411, 437, 442, 498,
503, 513, 518, 574) agora trazem a mensagem "Identifier::impl() nulo: Identifier.h:96 não tem
asserção, o C++ desreferencia o StringImpl* nulo (UB)". Conferido no upstream: `Identifier::impl()`
(`runtime/Identifier.h:96`) só devolve `m_string.impl()`, sem `ASSERT`, e `VariableEnvironment.h:175`
e `:311` (e os `declarePrivate*`) desreferenciam o resultado. Não há asserção a citar, então a
mensagem diz isso. Os `expect` de `parser_part2.rs` (499-803) também foram padronizados: `current_scope` (Parser.h:1270),
`current_variable_scope` (1277), `current_lexical_declaration_scope` (1285), `declare_variable` (1446),
os dois do catch (1461 e 1480) e `has_declared_parameter` (1510) citam "sem ASSERT em Parser.h:LINHA"
porque o C++ desreferencia `containingScope()` sem conferir; `pop_scope_internal` cita
`ASSERT(m_scopeStack.size() > 1)` (1386, desreferência em 1388) e `pop_scope_cleanup` cita
`RELEASE_ASSERT(cleanupScope.isValid())` (1428).
