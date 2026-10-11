# Auditoria do interpretador do Yarr

Escopo: `upstream/JavaScriptCore/yarr/YarrInterpreter.cpp` contra `src/yarr/yarr_interpreter*.rs`.
Auditoria curta (cerca de 5 minutos), sem compilar nem testar (o integrador compila).

## Conferido, sem divergência

- `InputStream` inteiro (`readChecked`, `readCheckedDontAdvance`, `tryReadBackward`,
  `readSurrogatePairChecked`, `reread`, `checkInput`/`uncheckInput`, `atStart`/`atEnd`, aritmética
  com wrap de `unsigned`).
- `testCharacterClass` (tabela, tabela latin1, limiar 6 para busca binária, `midpoint`, subtração
  `int` das buscas binárias).
- `checkCharacter`, `checkSurrogatePair`, `checkCasedCharacter`, `checkCharacterClass`,
  `checkCharacterClassDontAdvanceInputForNonBMP`.
- `tryConsumeBackReference`, `matchBackReference`, `backtrackBackReference` (incluindo grupos
  nomeados duplicados, referência a grupo ainda aberto, direção `Backward`, case-insensitive legado
  contra Unicode/UCS2).
- `matchDisjunction` (verificação de recursão, `remainingMatchCount` com `MATCH_LIMIT` de
  100000000, `ErrorHitLimit`), `matchNonZeroDisjunction`, `interpret`, `ByteCompiler::compile`
  (`TooManyDisjunctions`), `matchParentheses`, `refill`/`extend` dos contextos de parênteses.

## Divergência encontrada e corrigida

**Limite de memória dos contextos de retrocesso (`Options::maxRegExpStackSize`, 192 MB).**
No C++, `allocParenthesesDisjunctionContext` devolve `nullptr` quando `ensureCapacity` do
`BumpPointerPool` estoura o orçamento, e os chamadores (`refillParenthesesContextsToMinCount`,
`extendParenthesesContextsToMaxCount` e o ramo não guloso de `backtrackParentheses`) devolvem
`JSRegExpResult::ErrorNoMemory`. O porte usava `Vec` sem limite: um padrão patológico
(parênteses aninhados com quantificador grande) consumia memória do processo em vez de falhar
como o JSC.

Correção:
- `yarr_interpreter_cpp1.rs`: campo `context_bytes` no `Interpreter`, constante
  `MAX_REG_EXP_STACK_SIZE`, contabilidade em `alloc_*`/`free_*` (os `free_*` subtraem tudo que o
  `truncate` descarta), e `alloc_parentheses_disjunction_context` passa a devolver
  `Option<usize>` (o limite é conferido antes de `ParenthesesDisjunctionContext::new`, que já
  altera o `output`, igual ao C++, onde `ensureCapacity` falha antes de construir).
- `yarr_interpreter_cpp3.rs`: os três chamadores devolvem `ErrorNoMemory` no `None`.
- `yarr_interpreter_cpp4.rs`: inicialização de `context_bytes: 0`.

Limitação conhecida: os tamanhos por contexto são uma aproximação do `allocationSize` do build
de release (16 + 8 por quadro; 24 + 4 por backup id, alinhado a 8), e o `BumpPointerPool` real
conta páginas de overflow, então o ponto exato de estouro pode diferir em alguns por cento. O
contexto raiz de `interpret` não é limitado (no C++ o `nullptr` ali devolve `offsetNoMatch`).

## Não auditado neste passe

`matchDisjunction` termo a termo (cpp4, cerca de 500 linhas), `backtrackParentheses` (ramos
guloso/não guloso), `matchDotStarEnclosure`, lookbehind (`ParentheticalAssertion*`), `ByteCompiler`
(`emitDisjunction`, `atomParentheses*`) e o dump. Recomendado um segundo passe nessas regiões.

## Segundo passe (sem divergência nova)

Conferido linha a linha contra `YarrInterpreter.cpp`, sem achar divergência (nenhum arquivo `.rs`
alterado neste passe):

- `matchDisjunction` termo a termo, avanço e retrocesso (cpp4): entrada com `btrack`, `matchBegin`,
  `PatternCharacter*` e `PatternCasedCharacter*` nas direções `Forward` e `Backward` (inclusive o
  ramo não BMP com par substituto e a aritmética `unsigned` com wrap das posições), `Alternative*`
  (o `offset` do `BackTrackInfoAlternative` estendido com sinal e relido como `unsigned`),
  `BodyAlternative*` (avanço do início, `sticky`, `onceThrough`), `CheckInput`/`UncheckInput`/
  `HaveCheckedInput`, `DotStarEnclosure` (só no avanço), `matchNonZeroDisjunction`.
- `backtrackParentheses` (cpp3): `FixedCount`, `Greedy` (RepeatMatcher: `matchAmount <= min` usa
  `matchDisjunction`, acima disso `matchNonZeroDisjunction`; refill e extend quando abaixo do mínimo)
  e `NonGreedy` (tentativa de mais uma iteração, laço de retrocesso, refill ao mínimo), mais
  `refillParenthesesContextsToMinCount` e `extendParenthesesContextsToMaxCount`.
- `matchDotStarEnclosure`: `bolUnsatisfiable`, `noNewlineBefore`, ramo `dotAll` (com `/m`), volta
  até o início da linha, `anchors_eol` sem `/m`.
- `ParentheticalAssertion{Begin,End}` e seus retrocessos (cpp3), inclusive o reset das capturas, o
  `parenthesesWidth` e o `setPos(begin)` só no `Backward` do `backtrackParentheticalAssertionBegin`;
  `BackTrackInfoParentheticalAssertion::beginIndex()` é 0, igual ao C++.
- `ByteCompiler` (cpp5): `atomPatternCharacter`, `atomCharacterClass`, `atomBackReference`,
  `atomParentheses{Once,Terminal,Subpattern}{Begin,End}` (o `Subpattern` nasce `OnceBegin` e é
  consertado no fim), `atomParentheticalAssertion{Begin,End}`, `closeAlternative`,
  `closeBodyAlternative`, `alternative*Disjunction` e `emitDisjunction` completo (contagem de
  entrada checada nos dois sentidos, `backwardUncheckAmount`, lookahead com `uncheckAmount`,
  lookbehind com `haveCheckedInput` e `checkedCountForLookbehind`, trechos de `OffsetTooLarge`).
  `parenIds.duplicateNamedGroupId` do C++ corresponde ao `atom.second_id` do porte.

Ainda não auditado: o dump (`ByteTermDumper`, cpp6) e `matchParenthesesOnce*`/`Terminal*` já cobertos
só pelo passe anterior de forma resumida.
