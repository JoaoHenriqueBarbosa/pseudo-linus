# Regra da coluna de cada frame em Error.stack no bun 1.4.2

## Conclusão

O fork do JSC do bun (oven-sh/WebKit, branch main) NÃO muda a posição de chamada. O divot de uma
chamada é o mesmo do JSC puro. A coluna que o bun mostra no `Error.stack` vem de outra etapa: o bun
transpila todo fonte (inclusive .js) e **remapeia a posição do JSC pelo source map do transpilador**.
O JSC informa a coluna do divot da chamada; o bun troca por "posição original do segmento de mapping
que cobre aquela coluna gerada". Por isso o resultado parece "início do nome do callee".

## Passo 1: o que o JSC (fork) entrega, idêntico ao upstream

Fonte (oven-sh/WebKit, Source/JavaScriptCore):

- `parser/Parser.cpp`, caso `OPENPAREN` de `parseMemberExpression`:
  ```cpp
  JSTextPosition expressionEnd = lastTokenEndPosition();   // fim do callee, antes de "("
  ...
  base = context.makeFunctionCallNode(location, base, previousBaseWasSuper, arguments, expressionStart,
      expressionEnd, lastTokenEndPosition(), ...);          // (divotStart, divot, divotEnd)
  ```
  Ou seja, divot da chamada = offset logo depois do último token do callee = posição do `(`.
- `bytecompiler/BytecodeGenerator.cpp` `emitCall` (linha ~3999): `emitExpressionInfo(divot, divotStart, divotEnd);`
- `runtime/StackFrame.cpp:247` `computeLineAndColumn` -> `CodeBlock::lineColumnForBytecodeIndex` ->
  `ExpressionInfo::lineColumnInTextForInstPC` -> `provider.lineColumnInTextForOffset(sourceOffset + divot)`.
- `parser/SourceProvider.h:228` `documentLineColumn`: coluna 1-based = `m_startPosition.column.oneBasedInt + inText.column`
  (primeira linha) ou `inText.column + 1` (demais). Logo a coluna crua é a do caractere NO divot (o `(`).
  Comparação com o diff do upstream do repo: o fork só difere de `upstream/JavaScriptCore` por mudanças
  alheias à posição de chamada (RegExp compartilhado, ReflectConstruct, etc.). Nada de 'BUN' sobre divot
  (único `BUN_SKIP_FAILING_ASSERTIONS` em NodesCodegen.cpp:341, irrelevante).

## Passo 2: o que o bun faz com isso

Fonte (oven-sh/bun, main; os caminhos hoje são `src/jsc/bindings/`):

- `FormatStackTraceForJS.cpp` ~linhas 340-390 (função `formatStackTrace`), pass 1 e pass 2:
  ```cpp
  originalLineColumns[i] = frame.computeLineAndColumn();          // coluna crua do JSC (divot)
  remappedFrame.position.column_zero_based = OrdinalNumber::fromOneBasedInt(originalLineColumns[i].column).zeroBasedInt();
  ...
  if (anyRemap) remappedFrames.remap(getBunVM());                 // source map (SavedSourceMap)
  ...
  displayColumn = originalColumn;
  if (remappedFrame.remapped) { displayColumn = remappedFrame.position.column(); ... }
  ```
  O texto `at fn (url:linha:coluna)` imprime `displayColumn.oneBasedInt()` (linha ~443).
- O remap é feito em Rust/Zig em `src/jsc/SavedSourceMap.rs` (e o módulo de sourcemap do bun); NÃO li
  esse arquivo nem o `js_printer` do bun por falta de tempo.
- `ErrorStackFrame.cpp` `Bun::getAdjustedPositionForBytecode` (usado por `JSCStackFrame::calculateSourcePositions`
  e `ZigException.cpp:146`, caminho de `error.line/column` e do inspetor/uncaught, não do texto de `.stack`)
  usa o mesmo divot e só ajusta `op_construct*`/`op_super_construct*` voltando `expr.startOffset` (aponta o `new`):
  ```cpp
  case op_construct: ... adjustPositionBackwards(pos, expr.startOffset, code);
  ```

## Regra (hipótese inicial; confirmada na seção seguinte)

1. Coluna crua do JSC = coluna 1-based do `(` da chamada (para `new`, ver acima).
2. O bun procura no source map do código transpilado o mapping de maior coluna gerada <= essa coluna
   crua (lookup "greatest lower bound" na linha gerada) e exibe a coluna ORIGINAL desse mapping.
3. Como o printer só emite mapping no início de identificadores, literais e algumas pontas, o `(` cai
   no mapping anterior:
   - `o.p()`: o mapping anterior ao `(` é o do nome `p` -> coluna de `p`.
   - `ident()`: mapping do `ident` -> início de `ident`.
   - `o[k]()`: mapping de `k` (o `]` não tem mapping) -> primeiro token do subscript.
   - `o.a[o.a.length-1]()`: último mapping antes do `(` é o literal `1` -> o `1`.
   - `f()()`: o `)` do primeiro `()` tem mapping próprio (fim da chamada interna) -> coluna desse `)`.
4. Consequência para o porte: para reproduzir o bun, a coluna depende dos pontos de mapping do printer
   do bun (um mapping por identificador/literal/`)` de chamada), não do divot do JSC. Regra operacional
   prática: coluna = início do ÚLTIMO TOKEN-COM-MAPPING que termina antes do `(` da chamada, onde
   contam identificador, nome de propriedade, literal numérico/string e o `)` de uma chamada anterior;
   `]`, `.` e `(` não contam.

## Confirmado no fonte do bun (js_printer e sourcemap, 2026-10-08)

Lookup (`src/jsc/SavedSourceMap.rs::resolve_mapping` -> `ParsedSourceMap::find_mapping` ->
`src/sourcemap/Mapping.rs::find_index_from_generated`, e `InternalSourceMap::find`): busca binária pelo
último mapping com `(linha gerada, coluna gerada) <= (linha, coluna)` e que esteja NA MESMA linha gerada
(`lines == line`). É o "greatest lower bound" da hipótese. Sem mapping na linha antes do alvo, devolve `None`
e o frame fica com a coluna crua. O `(` não tem mapping próprio, então o alvo cai no mapping anterior.

Pontos em que `src/js_printer/lib.rs` chama `add_source_mapping` (início do token impresso):

- `ECall`: só o `)` final (`e.close_paren_loc`). NÃO mapeia o `(`, nem o agrupamento `(` extra.
- `ENew`: o `new` (`expr.loc`) e o `)` (`close_parens_loc`).
- `EDot`: o NOME da propriedade (`e.name_loc`); o `.` e o `?.` não mapeiam.
- `EIndex`: o início do índice (`e.index.loc`, depois do `[`); `[` e `]` NÃO mapeiam (índice privado: o nome).
- `EIdentifier`, `EImportIdentifier`, `EString`, `ENumber`, `EBigInt`, `ERegExp`, `EBoolean`, `ENull`,
  `EUndefined`, `EThis`, `ESuper`, `ENewTarget`, `EImportMeta`: `expr.loc`.
- `EArray`: `[` (`expr.loc`) e `]` (`close_bracket_loc`). `EObject`: `{` e `}` (`close_brace_loc`).
- `EFunction` (`function`, mais o nome), `EArrow` (só `async`; o `(` dos parâmetros mapeia via
  `open_paren_loc`), `EClass` (`class`, nome, `body_loc`, `close_brace_loc`), `print_block` (`{` e `}`).
- `ETemplate`: o início (a crase, ou a tag); partes `${}` não mapeiam por si, mas as expressões dentro sim.
- `EAwait`, `EYield`, `ESpread` (`...`), `EUnary` (palavra-chave sempre; operador só se prefixo).
- Não mapeiam: operadores binários, `,`, `;`, `:`, `=>`, `(` de qualquer tipo (exceto parâmetros de função),
  `)` de agrupamento, `]` de índice. Os comandos (`stmt.loc`) mapeiam a palavra-chave (`return`, `if`...).

A hipótese está confirmada, com a regra operacional: coluna exibida = coluna original do último token
mapeável cujo início é <= o `(` (divot), na mesma linha gerada. O porte reproduz isso em
`src/runtime/stack_frame.rs::callee_back_offset` (ver lá as divergências: linha mantida, classe do token
anterior decide regexp/divisão, array/índice e `)` de chamada/agrupamento).

Ainda não verificado no bun real: o passo 1 (coluna crua = `(`) com `--no-transpile`, e o caso de o printer
partir/juntar linhas (muda a linha exibida, não só a coluna).
