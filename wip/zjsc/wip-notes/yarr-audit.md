# Auditoria do Yarr contra upstream/JavaScriptCore/yarr

## Fatia 1 (2026-10-08): YarrParser.h

Lido linha a linha (C++ 1 a 2280 contra `yarr_parser.rs` e `yarr_parser_part2.rs`), mais
`YarrErrorCode.{h,cpp}` e `YarrFlags.{h,cpp}`:

- `NamedCaptureGroups` (nextAlternative, pushParenthesis, popParenthesis, add, reset).
- `CharacterClassParserDelegate` (máquina de estados, `m_isUnicode` só para `Unicode`, não `UnicodeSets`).
- `ClassSetParserDelegate` (união, interseção, subtração, `nestedClassBegin/End`, `canTakeSetOperand`,
  `computeMayContainStrings`, `end`), `ClassStringDisjunctionParserDelegate` (`\q{...}`).
- `isIdentityEscapeAnError` (listas de caracteres por modo), `parseEscape` inteiro (`\b \B \d..\W \0 \1-9`
  octal legado e backreference, `\c` com Annex B, `\x`, `\k<name>`, `\p \P`, `\q`, `\u`, identity escape,
  `\-` em classe unicode).
- `consumePossibleSurrogatePair`, `consumeAndCheckIfValidClassSetCharacter`, `parseCharacterClass`,
  `parseClassSet`, `parseClassStringDisjunction`, `parseParenthesesBegin` (grupos nomeados, lookbehind,
  modificadores `(?ims-ims:...)`), `parseParenthesesEnd`, `parseQuantifier`, `parseTokens`,
  `handleIllegalReferences`, `resetForReparsing`, `tryConsumeUnicodeEscape`, `tryConsumeGroupName`,
  `tryConsumeUnicodePropertyExpression`, `consumeNumber/64`, `consumeOctal`, `tryConsumeHex`.
- Tabela de mensagens de `ErrorCode` e ordem do enum, `hasHardError`, `parseFlags`/`flagsString`,
  `JSC_REGEXP_MOD_FLAGS` (i, m, s), máscaras `U_GC_*` (L=1..5, Mn=6, Mc=8, Nd=9, Pc=22),
  `char_category` para 0xFFFFFFFF (devolve 0, como o ICU).

Resultado: nenhuma divergência encontrada no parser. Foram adicionados testes de regressão em
`yarr_parser_part2.rs` (`mod tests`) fixando ordem de verificações e erros: octal e backreference legado
vs `u`, `\c`, intervalos em classe, `\u{..}`, `\p`, grupos nomeados (duplicados em alternativas), `\k`,
modificadores, lookbehind, quantificadores, v-flag (união, interseção, subtração, `\q{}`, negação com
strings, `\&` só em `v`). Os testes não foram executados (agente sem cargo): rodar
`cargo test yarr_parser` na primeira oportunidade.

Pontos de atenção para quem rodar os testes: o caso `\p{Not_A_Property}` depende de
`unicode_match_property` (fatia YarrUnicodeProperties).

## Falta ler

- `YarrPattern.cpp` (`yarr_pattern*.rs`, `yarr_pattern_cpp1..6.rs`): `YarrPatternConstructor`, delegates
  de classe (strings, set ops), `ignoreCase`/canonicalização na construção, `resetForReparsing`.
- `YarrInterpreter.cpp` (`yarr_interpreter*.rs`).
- `YarrUnicodeProperties.cpp` (`yarr_unicode_properties.rs`, `unicode_pattern_tables.rs`): tabelas de
  propriedade, `unicodeMatchProperty/Value`, `characterClassMayContainStrings`.
- `YarrCanonicalize.{h,cpp}` (`yarr_canonicalize*.rs`).
- `YarrSyntaxChecker`, `YarrMatchingContextHolder`, `Yarr.h` (só vistos de relance).

## Fatia 2 (2026-10-08): YarrPattern.cpp, amostra de alto risco

Lido função a função, C++ contra `yarr_pattern_cpp1.rs` e `yarr_pattern_cpp6.rs`: `putChar`,
`putCharNonUnion`, `putUnicodeIgnoreCase`, `putRange` (caminho rápido ASCII só em UCS2, truncamento
`char16_t` no `CanonicalizeSet`, avanço de `info` pela tabela contígua), `atomPatternCharacter`
(canonicalização Unicode vs UCS2), `atomBuiltInCharacterClass` e `atomCharacterClassBuiltIn` (word com
ignoreCase unicode, propriedades com fechamento sob case, `\P` com núcleo invertido, classes com
strings), `atomCharacterClassEnd`, `atomBackReference`, `atomNamedBackReference` (duplicatas, lookbehind
mais externo), `atomNamedForwardReference`, `quantifyAtom` (divisão min/max para frente e para trás,
`isCopy`, referências pendentes em lookbehind, `{0}`, asserção quantificada).

Resultado: nenhuma divergência de comportamento nessas funções; nenhuma edição de código.

Ainda não lido nesta fatia (continua pendente): `performSetOpWith*` e `unionStrings/intersectionStrings/
subtractionStrings`, `addSortedRange`/`mergeRangesFrom`, `coalesceTables`, `checkForTerminalParentheses`,
`optimizePossessiveQuantifiers`, `optimizeBOL`, `factorAlternatives`/`wrapAlternativesForDispatch`,
`optimizeDotStarWrappedExpressions`, `extractSpecificPattern`, `FirstCharacterBitmapBuilder`,
`setupOffsets`, `registerCopiedForwardReferences`/`resolveForwardReferencesInLookbehindTo`.

## Fatia 3 (2026-10-08): YarrPattern.cpp, o pendente da fatia 2

Lido função a função, C++ contra `yarr_pattern_cpp1.rs` (conjuntos), `yarr_pattern_cpp2.rs`,
`yarr_pattern_cpp4.rs`, `yarr_pattern_cpp5.rs` e `yarr_pattern_cpp6.rs`:

- Operações de conjunto: `performSetOpWith` (as duas sobrecargas), `performSetOpWithStrings`,
  `performSetOpWithMatches`, `latin1Op`, `latin1Invert`, `nonLatin1OpSorted` (chunks de 2048, leitura
  das listas por índice, coalescência da faixa anterior, `canProduceMore` por operação),
  `nonLatin1Invert`, `unionStrings`/`intersectionStrings`/`subtractionStrings` (o `merge_strings` com
  três flags reproduz as três; sobras da lista esgotada seguem a região certa) e `compareUTF32Strings`.
- `addSortedRange` (as duas formas), `mergeRangesFrom`, `coalesceTables` (incluindo a remoção em corrida
  dos matches dentro da faixa e o `anyCharacter`).
- Otimizações: `checkForTerminalParentheses` (passos 1 e 2 da lista de strings, `unwrapSingleGroup`,
  corte das alternativas após a primeira vazia, marca `isTerminal`), `optimizePossessiveQuantifiers`
  (`termMatchesCharacter`, `followerForcesPossessive`), `recomputeStartsWithBOL`, `optimizeBOL`,
  `factorAlternatives` (barreiras, corrida mínima de 8, ordenação estável, `mergeSharedPrefix`,
  orçamento cobrado sobre o tamanho antes de mover, refatoração de grupos aninhados com as mesmas
  condições), `wrapAlternativesForDispatch` (limiares, `DotStarEnclosure`, intervalo de captura,
  `startsWithBOL` do invólucro), `optimizeDotStarWrappedExpressions`, `termsMayMatchNewline`,
  `classContainsCodePoint`, `extractSpecificPattern` (átomo, espaços, quebras de linha).
- `setupOffsets` (só o repasse inicial `(body, 0, 0)`), `setupNamedCaptures`,
  `computeEndAnchoredFixedSize`, a ordem das passadas em `YarrPattern::compile`.
- `FirstCharacterBitmapBuilder` e `computeFirstCharacterBitmap`: mesma desistência (`gaveUp`) em direção
  para trás, em `DotStarEnclosure` depois de um termo que consome, profundidade acima de 8 e disjunção
  de topo que casa vazio; mesma pré-condição por flags (`sticky`, `global`, `multiline`, modificadores,
  `^` inicial literal em toda alternativa).
- `registerCopiedForwardReferences` e `resolveForwardReferencesInLookbehindTo` (`namesThisGroup` ignora a
  posição 0 do vetor de ids, `containsBackreferences` ligado só quando converte).

Resultado: nenhuma divergência de comportamento; nenhuma edição de código.

Não lido ainda: o corpo de `setupDisjunctionOffsets` (só o chamador foi conferido), `copyDisjunction`/
`copyTerms` (usados por `optimizeBOL`) e a ordem exata de `m_disjunctions` depois de `factorAlternatives`.
Esta última é a única diferença estrutural conhecida: no Rust as disjunções novas entram no vetor ao serem
criadas (precisam de id) e não ao fim, como no C++ (`optimizePossessiveQuantifiers` itera
`m_disjunctions`, mas só marca flags por termo, então a ordem não altera o resultado). Vale um teste de
conformidade em lote contra o oráculo (padrões com 8 ou mais alternativas com prefixo comum, `/i`, grupos
aninhados e lookbehind com referência para frente), já que esta fatia foi só leitura.

## Auditoria de YarrUnicodeProperties e YarrCanonicalize

Escopo: `yarr_unicode_properties.rs`, `unicode_pattern_tables.rs`, `yarr_canonicalize.rs`,
`yarr_canonicalize_unicode.rs`, contra `YarrUnicodeProperties.{h,cpp}`, `YarrCanonicalize.h`,
`YarrCanonicalizeUCS2.cpp` e os arquivos derivados em `derived/JavaScriptCore/yarr`.

Conferido, sem divergência:

- `unicode_match_property_value`: nomes `Script`/`sc`, `Script_Extensions`/`scx`, `General_Category`/`gc`;
  qualquer outro nome de propriedade devolve `None`, como no C++ (valor inválido também).
- `unicode_match_property`: ordem binárias, categoria geral (valor sozinho, `Lu`, `Letter`...), e só
  em `CompileMode::UnicodeSets` as propriedades de sequência (v-flag). Fora da v-flag `RGI_Emoji` falha.
- Tabelas de hash: 895 pares (chave, índice) idênticos aos de `UnicodePatternTables.h`, 1567 entradas
  de `HashIndex` idênticas, 374 classes com a mesma `CharacterClassWidths`, e o corte de
  `unicode_character_class_may_contain_strings` (367..=373, `>= 374` falso).
- Classes de strings: `in_canonical_form = true` como `m_inCanonicalForm = true`.
- `UCS2_RANGE_INFO`: 460 faixas idênticas às de `YarrCanonicalizeUCS2.cpp`; `LATIN1_CANONICALIZATION_TABLE`
  idêntica (hash dos valores).
- `UNICODE_RANGE_INFO`: 548 faixas idênticas ao `YarrCanonicalizeUnicode.cpp` derivado, e verificadas de forma
  independente contra `ucd/CaseFolding.txt` (classes de equivalência de case folding simples, status C e S,
  nunca F nem T): 0 divergências em todos os pontos de código até 0x10FFFF. Ou seja, ignoreCase com `u`/`v`
  usa case folding simples, e o modo UCS2 usa o `toUpperCase` simples do ES6 (K e ſ só equivalem a k e s no
  modo Unicode).
- `canonical_range_info_for`, `get_canonical_pair`, `is_canonically_unique`, `are_canonically_equivalent`:
  mesma lógica do .h (aritmética com wrapping equivalente ao `unsigned`).

Nenhuma correção foi necessária. Não foi rodado cargo (regra do agente); as verificações foram feitas
por script Python sobre os fontes. Observação: `String::hash` precisa coincidir com o `StringHasher` do WTF
(mesmo hash para latin1 e UTF-16), senão `HashTable::entry` falha em silêncio; vale um teste na fatia WTF.
