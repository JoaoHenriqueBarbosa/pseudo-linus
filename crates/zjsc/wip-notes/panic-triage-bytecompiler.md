# Triagem de panic em src/bytecompiler e src/bytecode

Critério: (a) invariante que o C++ garante com RELEASE_ASSERT/ASSERT/CRASH (ou desreferência sem
conferência), mantém; (b) caminho alcançável por JS do usuário em que o JSC lança erro, converte.

Método: `grep -rnE 'unwrap\(|expect\(|panic!|unreachable!' src/bytecompiler src/bytecode` (936
ocorrências fora de linha de comentário: 733 `.unwrap()`, ~200 `.expect(..)`, ~40 `unreachable!`, 4
`panic!`). Os 8 arquivos fora de `bytecompiler/nodes_codegen*` e `bytecode_generator_cpp*` (e todos de
`src/bytecode`) foram lidos um a um. Os 700+ restantes caem em famílias de forma idêntica, lidas por
amostragem de cada família contra `upstream/JavaScriptCore/bytecompiler/{BytecodeGenerator,NodesCodegen}.cpp`.
Isto é uma triagem por família, não uma leitura linha a linha das 700.

Resultado: **nenhum caso (b)**. Nenhuma alteração de código e nenhum teste novo. Os limites que o JSC
transforma em erro para fonte de usuário já estão portados e nenhum deles é panic:

- `BytecodeGenerator::generate` (`bytecode_generator_cpp1.rs` 123, 391-401): `m_outOfMemoryDuringConstruction`,
  `finalize()` falho, bytecode maior que `INT32_MAX` e `m_expressionTooDeep` devolvem
  `ParserError::OutOfMemory`.
- `try_set_arguments_length` / `try_set_argument_offset` falhos (`bytecode_generator_cpp1.rs` 773, 787) marcam
  `out_of_memory_during_construction` em vez de abortar.
- Pilha esgotada: `emit_node*` checa `is_safe_to_recurse` e chama `emit_throw_expression_too_deep_exception`
  (`bytecode_generator_cpp5.rs` 981), que devolve `newTemporary()` como o C++ (BytecodeGenerator.cpp 4641), então
  os `.unwrap()` de registro depois de `emitNode` nunca veem `None` por causa de profundidade.
- `NumberNode`/`StringNode`/BigInt enorme sem valor: `emit_load_js_value` cai em
  `emit_throw_expression_too_deep_exception` (NodesCodegen.cpp, comentário "OOM").
- `break`/`continue`/`return` em lugar raro: validados pelo parser (SyntaxError), então
  `continue_target().expect("o laço sempre tem continueTarget")` e `label_scopes.last().unwrap()` só rodam com
  rótulo já resolvido, igual ao `ASSERT` do C++.

## (a) Invariantes, mantidos

| Família (arquivos) | Ponto | Asserção do upstream |
|---|---|---|
| `bytecode_generator.rs` 349 | `numberOfBreaksOrContinues` estoura u32 | `Checked<uint32_t, CrashOnOverflow>` (BytecodeGenerator.h 221); mais de 2^32 `break` num programa não cabe na memória |
| `bytecode_generator_cpp1.rs` 384 | `numCalleeLocals < FirstConstantRegisterIndex` (2^30 locais) | `RELEASE_ASSERT` em `generate` (BytecodeGenerator.cpp 370) |
| `bytecode_generator_cpp3.rs` 950 | `addVar` redeclarado com outro `VarKind` | `RELEASE_ASSERT_WITH_MESSAGE` "Trying to add variable called" (BytecodeGenerator.cpp 2618); o parser não deixa passar |
| `bytecode_generator_cpp2.rs` 213 | `iterator != end` no ambiente | `RELEASE_ASSERT` (BytecodeGenerator.cpp 2344/1169) |
| `bytecode_generator_cpp2.rs` 427 | `functionSymbolTable` | `RELEASE_ASSERT(functionSymbolTable)` (BytecodeGenerator.cpp 1328) |
| `bytecode_generator_cpp3.rs` 550-760, 872 | pilha lexical vazia, `symbol_table`/`scope` ausentes | `m_lexicalScopeStack.last()`, `RELEASE_ASSERT(loopSymbolTable)`, `RELEASE_ASSERT(stackEntry.m_scope)` (2457/2476) |
| `bytecode_generator_cpp3.rs` 657, 1091, `cpp4.rs` 609, `cpp1.rs` 217, 1141, `cpp5.rs` 796, 976 | ramos mortos | `RELEASE_ASSERT_NOT_REACHED()` (2407, 2620, 2714, 2726, 2755, 2787, 3299) |
| `bytecode_generator_cpp4.rs` 669, 2193 | `m_scopeNode->isFunctionNode()` | `RELEASE_ASSERT(m_scopeNode->isFunctionNode())` (3340) |
| `bytecode_generator_cpp4.rs` 812, 1106 | template object no set recém-inserido; construtor padrão builtin | `m_templateObjectDescriptorSet.add` (3451); `createDefaultConstructor` com fonte interna |
| `bytecode_generator_cpp5.rs` 283, 500, 816 | `takeLast`/`last()` de pilhas de escopo, try e switch | `m_lexicalScopeStack.takeLast()`, `m_tryContextStack.last()`, `m_switchContextStack.last()` (pilhas pareadas push/pop) |
| `bytecode_generator_cpp5.rs` 824-923 | clausulas de switch inteiras/string | `ASSERT(nodes[i]->isNumber())` / `isString()` guardados por `SwitchInfo::SwitchType` |
| `bytecode_generator_cpp6.rs` 492-496 | `state` do gerador estoura i32 | `Checked<int32_t>` com `CrashOnOverflow` na geração do `yieldPointIndex` |
| `bytecode_generator_cpp6.rs` 1096-1383 | `finallyContext`, `completionTypeRegister` | `ASSERT`/desreferência em `emitFinallyCompletion` (BytecodeGenerator.cpp 5733-5803) |
| `bytecode_generator_cpp6.rs` 572-1042, `cpp4/5` `emit_*().unwrap()` | `emit_get_by_id`, `emit_is_object`, `emit_load_js_value` com dst `Some` | o C++ usa o `RegisterID*` direto; só é nulo com `ignoredResult()`, que esses chamadores nunca passam |
| `bytecode_generator_cpp1c.rs`, `part2.rs`, `generator_base.rs` 186 | `top_level_scope_register`, `usingScopeStack`, `callee_locals.pop` | `ASSERT(!m_usingScopeStack.isEmpty())`; registro alocado antes no mesmo escopo; laço guardado por `last().is_some_and` |
| `bytecode_generatorification.rs` 167, 316 | identificador numérico, symbol table do quadro | `ASSERT(identifier.impl())`; `needsGeneratorification` implica a tabela |
| `nodes_codegen*.rs` `move_register(.., x.as_ref().unwrap())` (~450) | `RegisterID*` devolvido por `emitNode`/`emitCall` | desreferência crua no C++ (`base.get()`, `result.get()`) |
| `nodes_codegen*.rs` `unreachable!("esperava um XNode")`, `isSpreadExpression`, `isArrayLiteral`, `isResolveNode`, `isDotAccessorNode`, `isBracketAccessorNode` | downcast depois de predicado | `static_cast<...>` após `ASSERT(x->isY())` (NodesCodegen.cpp, ex.: 3475-3490) |
| `nodes_codegen_cpp1.rs` 27, 173, 678 | `emitNode` sem dst, flags de RegExp, `tryCreate` | `RELEASE_ASSERT(array)` (NodesCodegen.cpp 471), `ASSERT(flags)` (171), `emitNode(this)` não nulo sem dst |
| `nodes_codegen_cpp1b.rs` | `ASSERT(instanceElementDefinitions/staticElementDefinitions)`, `ASSERT(it != map.end())`, nome do acessor | `ASSERT` no `PropertyListNode::emitBytecode` |
| `nodes_codegen_cpp5b.rs` 32-407, 634, 656 | `as_*_node().unwrap()` do `lexpr`; cláusula número/string | `static_cast` após `isDestructuringNode()`/`isDotAccessorNode()`; `ASSERT(isNumber/isString)` do switch |
| `nodes_codegen_cpp3.rs` 292 | lista de argumentos vazia | `RELEASE_ASSERT(args->m_listNode)` |
| `nodes_codegen_dispatch.rs` 69 | `PrivateIdentifierNode::emitBytecode` | `RELEASE_ASSERT_NOT_REACHED()` |
| `bytecode/unlinked_code_block.rs` (`rare data`, `instructions`, `expression_info`) | acesso antes de existir/finalize | `m_rareData` criado antes por `ensureRareData`; `ASSERT(m_instructions)`; `m_expressionInfo` só lido depois de `finalize` |
| `bytecode/unlinked_code_block_generator.rs` 191, 204 | `last_mut` depois de `push` | tabela recém-inserida |
| `bytecode/code_block.rs` 160, 241, 253, 807-809, 953, 997, 1023-1214 | `callTypeFor`, `m_jitCode`, owner, `m_metadata`, `m_rareData` | `RELEASE_ASSERT_NOT_REACHED()`, `ASSERT(m_jitCode)`, `jsCast<FunctionExecutable*>`, `uncheckedDowncast`, `ASSERT(m_metadata)`, `RELEASE_ASSERT(m_rareData)` |
| `bytecode/bytecode_ops.rs` 577-618, `bytecode_ops_decode.rs` 317-375, `precise_jump_targets.rs` 48 | rótulo não por deslocamento, campo ausente | `ASSERT_NOT_REACHED()` em `setTargetLabel`/`BoundLabel` |
| `bytecode/fits.rs` 220 | enum com valor fora do conjunto ao decodificar operando | `ASSERT` de `Fits<Enum>::convert` sobre bytecode que o gerador mesmo escreveu |
| `bytecode/bytecode_use_def.rs` 375, 667, `bytecode_dumper.rs` 371, `expression_info.rs` 48, `virtual_register.rs` 204 | `op_wide16/32` sem decode, opcode sem dump, `FieldID` inválido | `RELEASE_ASSERT_NOT_REACHED()` |
| `bytecode/bytecode_intrinsic_registry.rs` 72, 80, `bytecode_intrinsic_constants.rs` 90 | variante errada da `Entry` | `ASSERT(m_type == ...)` em `BytecodeIntrinsicRegistry::Entry` |
| `bytecode/speculated_type.rs` 544, `watchpoint.rs` 493, `adaptive_inferred_property_value_watchpoint_base.rs` 98, `code_block_hash.rs` 92, `property_condition.rs` 182, `array_allocation_profile.rs` 45-49, `unlinked_function_executable.rs` 615, `bytecode_graph.rs` 43 | nome de especulação, set já inflado, set de substituição, provider, uid, `m_lastArray`, grafo sem blocos | `RELEASE_ASSERT_NOT_REACHED()` / `ASSERT` nas funções homônimas do C++ |

## Limites conferidos que NÃO são panic

- Restos de 2^32 (`as u32`/`as i32` em posições do `InstructionStreamWriter`) são cortados por
  `size > INT32_MAX` em `generate`, que devolve `OutOfMemory`.
- `emit_throw_out_of_memory_error` (`bytecode_generator_cpp5.rs` 675) é o intrínseco
  `@throwOutOfMemoryError` do JS embutido, não um panic.

## Pendência

Se algum teste futuro com fonte gigante (milhões de `break`, 2^30 locais) mostrar abort, o lugar de conferir
é a tabela acima contra `generate`; hoje nada disso cabe na memória do runner, então não há teste novo.
