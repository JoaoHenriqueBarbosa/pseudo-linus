# Auditoria de RegExp (2026-10-08)

## Golden novo

- `scripts/gen-regexp-golden.js` gera `tests/golden/regexp_v_bun.tsv` (4447 programas, medidos no bun 1.4.2; 4 descartados por não terminarem em 5 s).
- Teste: `tests/regexp_v_bun_golden.rs` (mesmo padrão de `function_error_bun_golden.rs`). Não foi compilado nem rodado.
- Cobertura: flags v (classes aninhadas, `&&`, `--`, `\q{}`, propriedades de strings), d, s, y, g com lastIndex, lookbehind variável, grupos nomeados duplicados, modificadores `(?i:...)`, backreferences nomeadas, cerca de 270 propriedades `\p{...}` (Script, scx, gc, binárias) por contagem de pontos de código em 0..0x2FFFF, case folding u/v/i, quantificadores enormes, backtracking limitado, `Symbol.replace/split/match/search/matchAll` com subclasses e exec customizado, `replaceAll`/`matchAll`, `RegExp.escape`, `source`/`flags`/`toString`, e cerca de 250 SyntaxError com mensagem exata.
- Atenção: o teste varre até 0x30000 pontos de código por propriedade (cerca de 300 programas assim); em build debug pode ser lento.

## Auditoria de src/yarr contra upstream/JavaScriptCore/yarr

- `yarr_error_code.rs`: as 38 mensagens e a ordem batem com `YarrErrorCode.cpp`.
- `yarr_flags.rs`: `parseFlags` (duplicata, `u` com `v`, caractere desconhecido) bate com `YarrFlags.cpp`.
- Sem divergência óbvia encontrada nesses dois. O restante (parser, pattern, interpreter) não foi lido linha a linha no tempo disponível: fica para depois que o golden novo rodar e apontar falhas, que são o roteiro mais barato para achar divergências.

## Simulação à mão contra o golden (2026-10-08, sem compilar)

Lidos linha a linha contra `upstream/JavaScriptCore/yarr/`: `ClassSetParserDelegate` inteiro, `parseClassSet`, `parseClassStringDisjunction` (início), `NamedCaptureGroups`, ramo `(?<` e modificadores de `parseParenthesesBegin`, `isIdentityEscapeAnError`, caso `q`/`p`/`P` de `parseEscape`, `consumeAndCheckIfValidClassSetCharacter`, `atomCharacterClass{Begin,PushNested,PopNested,End,SetOp}`, `atomParentheticalModifierBegin`, `CharacterClassConstructor::{putChar,putCharNonUnion,putUnicodeIgnoreCase,putRange,atomClassStringDisjunction}`, `unicodeMatchProperty{,Value}`. Nenhuma divergência encontrada; nada foi alterado no código.

Programas simulados (golden `regexp_v_bun.tsv` e `regexp_more_bun.tsv`), resultado esperado do porte igual ao do bun:

1. `[a-c&&b-d]` v: `a-c` fecha em `AfterSetRange` e troca Default por Union; `&&` cai em `set_intersection_op` com op Union, erro `InvalidClassSetOperation`. Igual.
2. `[ab&&c]` v: `b` em `CachedCharacter` descarrega `a` e troca para Union; `&&` erra. Igual.
3. `[a&&b]` v em `"()"`: `&&` em estado `CachedCharacter` com op Default passa; `b` entra por `put_char_non_union` (interseção); resultado nulo. Igual.
4. `[[a-z]--[aeiou]]` v em `"abc"`: `pop_nested` faz a subtração com o construtor aninhado; casa `b` no índice 1. Igual.
5. `[^\q{a}]` v: `\q{a}` tem uma só string de 1 ponto, `may_contain_strings` falso, sem erro. Igual.
6. `[^[\p{RGI_Emoji}]]` v: `nested_class_end` com `inverted` e `may_contain_strings` dá `NegatedClassSetMayContainStrings`. Igual.
7. `[\P{RGI_Emoji}]` v: caso `P` com `character_class_may_contain_strings` dá o mesmo erro. Igual.
8. `\p{RGI_Emoji}` com `u` (não v): `unicode_match_property` só consulta a tabela de sequências em `UnicodeSets`; erro de propriedade. Igual.
9. `[\q{aa|a}]` v em `"😀"`: `\q` vira a string `aa` mais o caractere `a`; nulo. Igual.
10. `[\q{}]` v em `"A"`: string vazia, casa vazio no índice 0 (`{"a":[""],"i":0}`); depende de `expand_class_with_strings` emitir o membro vazio. Conferido só na leitura do ponto de entrada, o corpo da expansão e do interpretador ficou sem leitura.
11. `[\q{ss}]` u: modo `CharacterClass`, `q` cai em `is_identity_escape_an_error`, `InvalidIdentityEscape`. Igual.
12. `(?<a>x)(?<a>y)`: `add` devolve falso na mesma alternativa, `DuplicateGroupName`. Igual.
13. `(?<a>a)(?:|(?<a>b))`: `push_parenthesis` semeia a pilha ativa com `{a}`, a segunda alternativa herda `{a}` em `next_alternative`, erro. Igual.
14. `(?<a>a)|b|(?<a>c)` d em `"c"`: `next_alternative` no nível 0 zera o conjunto ativo sem semear; nome novo, sem erro. Igual.
15. `(?:(?<a>a)|(?<a>b))+` em `"x"`: dentro dos parênteses o semeio vem de `{}` do pai; sem erro. Igual.
16. `(?ims-ims:a)`: `set.contains_any(unset)` dá `InvalidRegularExpressionModifier`. Igual.
17. `(?imsi:a)` u: duplicata em `set`, mesmo erro. Igual.
18. `(?-m:^b)` com `m` em `"a\nb"`: `flags` fica sem multiline durante o grupo e é restaurado no `atom_parentheses_end` (contexto marcado como modificador). Só a metade do parser e do `atomParentheticalModifierBegin` foi lida; a restauração em `atomParenthesesEnd` e o uso de `flags` por termo no interpretador ficaram sem leitura.
19. `(?i:\u212a)` u em `"K"`: modo canônico Unicode, `put_char` consulta `canonical_range_info_for`, que devolve o conjunto com `k`, `K` e U+212A; casa. A correção depende do conteúdo da tabela de `yarr_canonicalize_unicode.rs`, que não foi conferido contra `YarrCanonicalizeUnicode.cpp`.
20. `\W` iu em `"\u017f"` e `"\u212a"`: `atom_character_class_built_in` escolhe `nonword_unicode_ignore_case_char_character_class`; nulo. Igual na lógica; a tabela dessa classe não foi conferida.
21. `[a-z]` i u e v: `put_range` pula o atalho ASCII (modo canônico Unicode) e passa pela tabela, incluindo o ramo `++info` reescrito como nova consulta em `info.end + 1`. Equivalente enquanto a tabela for contígua.
22. `(?<=(a+))b` com e sem u em `"aab"`: captura gulosa para trás, `["b","aa"]`. Depende do interpretador (`MatchDirection::Backward`), não lido.
23. `(?<=(a)\1)b` em `"aab"`: o esperado é `["b","a"]` (backreference avaliada antes da captura, vazia). Depende do interpretador, não lido.
24. `\p{scx=Cherokee}` u: `unicode_match_property_value` com `scx` usa a tabela de extensões de script. Igual no despacho; os dados em `unicode_pattern_tables.rs` não foram conferidos contra a UCD.
25. `[\q{ab}x]` v: caractere depois de operando `AfterSetOperand` não descarrega caractere em cache (a correção já existe no porte e no upstream local).

### O que ficou sem leitura (próximo passo, em ordem de risco)

- `yarr_interpreter*.rs` (lookbehind com backreference, `MatchDirection::Backward`, backtracking limitado, grupos nomeados duplicados em `groups`/`indices.groups`, retorno de grupos não participantes como `null`).
- `expand_class_with_strings` e `factorAlternatives` (casos 10 e `\p{RGI_Emoji}+`).
- Tabelas: `yarr_canonicalize_unicode.rs`, `unicode_pattern_tables.rs` e `reg_exp_jit_tables.rs` contra os `.cpp` gerados (amostrar U+017F, U+212A, U+1E9E, U+0130, U+0131).
- Camada JS (`Symbol.replace/split/matchAll`, `RegExp.escape`, `source`/`flags`), fora de `src/yarr`.

## Tabelas Unicode: amostragem contra o bun (segunda passada)

- `scripts/gen-regexp-tables-golden.js` gera `tests/golden/regexp_tables_bun.tsv` (889 linhas, cerca de 1 MB, sem caminho da máquina); teste em `tests/regexp_tables_bun_golden.rs`. Não compilado nem rodado.
- Case folding: cada linha testa 25 pontos X, com `/\u{X}/iu` e `/\u{X}/iv` contra os candidatos que o bun diz equivalentes (mais X-1 e X+1 como iscas). Amostra: todo ponto com maiúscula ou minúscula distinta em 0..0x2FFFF, passo 61, fronteiras de cada bloco de 0x100, e os especiais (U+0130, 0131, 017F, 1E9E, 212A, 2126, 03C2, 01C5, 0345, 1FBE).
- Propriedades: 275 expressões válidas no bun (Script e sc, Script_Extensions e scx, General_Category e gc, binárias, nomes longos e curtos), em `u` e `v`, cada uma com `\p` e `\P` nas fronteiras de intervalo (primeiro, último e vizinhos; no máximo 40 intervalos por propriedade, os demais por amostragem uniforme).
- Conferência por leitura, sem cargo: `UNICODE_RANGE_INFO` (548 faixas) e os 27 conjuntos foram parseados direto do `.rs` por script do bun e comparados ao `/iu` e `/iv` do bun para todo ponto com case em 0..0x2FFFF: 0 divergências; as faixas cobrem 0..0x10FFFF sem lacuna; nenhum ponto sem case tem tipo diferente de `CanonicalizeUnique` em todo o espaço Unicode. Esta parte da tabela de `yarr_canonicalize_unicode.rs` fica conferida.
- `unicode_pattern_tables.rs`, `yarr_unicode_properties.rs` e `reg_exp_jit_tables.rs`: sem divergência encontrada por leitura de fronteiras porque o upstream local não traz o `UnicodePatternTables.h` gerado (só o gerador `.py`); a verificação fica a cargo do golden novo, a rodar. Nenhum arquivo de `src/` foi editado.

## Leitura do interpretador, `expand_class_with_strings` e `factor_alternatives` (2026-10-08, segunda passada, sem compilar)

Lidos contra `YarrInterpreter.cpp` e `YarrPattern.cpp` do upstream local: `tryConsumeBackReference`, `matchBackReference`, `backtrackBackReference` (inclusive o ramo `Backward` do guloso e o grupo nomeado duplicado), `recordParenthesesMatch`, `resetMatches`, `matchParenthesesOnce{Begin,End}` e os `backtrack*`, `matchParenthesesTerminal{Begin,End}` e os `backtrack*`, `matchParentheticalAssertion*` e os `backtrack*`, `ParenthesesDisjunctionContext` (backup e restauração de `output` e das posições de grupo duplicado), `allocParenthesesDisjunctionContext`, `interpret()` (inicialização de `output`), `atomParenthesesOnceEnd`/`TerminalEnd` (troca de `inputPosition` para casamento Backward, `duplicateNamedGroupId`), `offsetForDuplicateNamedGroupId`, `expandClassWithStrings`, `factorAlternatives`, `mergeSharedPrefix`, `firstLiteralCharacter`, `isSameLiteralTerm`, `accumulateCaptureRange`, `clearTerminalMarks`, constantes (`alternationFactoringMinRun` 8, `alternationDispatchMin*` 4 e 12, orçamento 1<<16 mais 16 por termo). Nenhuma divergência encontrada; nada foi alterado no código. As diferenças são só de modelo de posse (índices no lugar de ponteiros, `BTreeSet` no lugar de `BitVector`, ambos em ordem crescente) e a verificação de limite de pilha antes de construir o contexto, que preserva a ordem observável.

Programas simulados à mão (valores confirmados no bun 1.4.2; os de lookbehind e grupo nomeado têm igual no golden):

26. `(?<=(a+))b` em `"aab"`: `["b","aa"]`, índice 2. Captura gulosa para trás, `record_parentheses_match` grava fim e depois início. Igual.
27. `(?<=(a)\1)b` em `"aab"`: `["b","a"]`. Para trás a avaliação vai da direita para a esquerda: `\1` é o primeiro termo avaliado, vê `OFFSET_NO_MATCH` e casa vazio (ramo `match_end == OFFSET_NO_MATCH` de `match_back_reference`). Igual no porte.
28. `(?<=\1(a))b`: `["b","a"]`, aqui `(a)` é avaliado antes e `\1` casa `a` para trás (`try_consume_back_reference` com `Backward` e `uncheck_input(match_size)` no fim). Igual.
29. `(?<=(?<n>a)\k<n>)b`: `groups.n = "a"`. Igual (mesmo caminho do 27).
30. `(?<!(a))b` em `"cb"`: `["b", undefined]`. `match_parenthetical_assertion_end` invertido zera `output` das capturas da faixa. Igual.
31. `(?<=([abc])+)d` em `"abcd"`: `["d","a"]` (última iteração para trás é `a`). Caminho `match_parentheses` com contexto por iteração. Igual na leitura.
32. `(?<=(\d+)(\d+))$` em `"1053"`: `["","1","053"]`. Guloso à direita consome primeiro, backtracking via `backtrack_parentheses_once*`. Igual.
33. `(?<a>a)|(?<a>b)` com d em `"b"`: `groups.a = "b"`, `indices.groups.a = [0,1]`, `m[1]` undefined. `match_parentheses_once_end` grava `output[offset]` com o id do subpadrão que casou; `interpret()` zera as posições de grupo duplicado. A montagem de `groups`/`indices.groups` fica na camada JS (`RegExpMatchesArray`), que não existe em `src` ainda: sem como conferir.
34. `(?:(?<a>x)|(?<a>y))\k<a>` em `"yy"`: `["yy",undefined,"y"]`. `match_back_reference` lê `output[offset_for_duplicate_named_group_id]`, depois `output[id<<1]`. Igual.
35. `(?:(?<a>x)|(?<a>y))+` com d em `"xy"`: `["xy",undefined,"y"]`, `indices.groups.a = [1,2]`. `ParenthesesDisjunctionContext::new` salva e zera `output` e a posição do grupo duplicado a cada iteração, `restore_output` desfaz no backtracking. Igual.
36. `(a)|b` com d em `"b"` e `(a)?(b)?` em `"b"`: grupos não participantes ficam `OFFSET_NO_MATCH` e viram `undefined` (só o `output[i<<1]` é limpo em `interpret()`, como no C++). Igual.
37. `(?:(a)|b)*` em `"ab"`: `["ab",undefined]`, a segunda iteração zera o grupo 1. Igual (contexto por iteração).
38. `(z)((a+)?(b+)?(c))*` em `"zaacbbbcac"`: `[..., "ac","a",undefined,"c"]` (spec: grupos internos reiniciam por iteração). Igual.
39. `[\q{abc|ab|a|}]` v em `"abd"`: `["ab"]`. `expand_class_with_strings` emite as strings na ordem do conjunto (mais longa primeiro), caractere único e por último o membro vazio; abaixo do piso (4 alternativas e 12 de tamanho total) não cria o grupo de despacho. Igual.
40. `[\q{ab|cd}x]+` v em `"abxcdab"`: casa tudo. Sem grupo de despacho, o `+` aplica ao grupo único. Igual.
41. `^[\q{}]$` v em `""`: `[""]`. Grupo com alternativa vazia (não gera alternativas; `has_empty_string && alternative_count != 0` falso, o grupo fica com a alternativa inicial vazia). Igual ao C++.
42. `[\q{abc|abd|abe|a}]` v em `"abe"`: `["abe"]`. 4 strings, total 10 < 12, sem despacho; `factor_alternatives` só age com corrida de 8 ou mais, então não toca. Igual.
43. `(?<=\k<a>(?<a>.))x` em `"aax"`: `["x","a"]`, `groups.a = "a"`. Igual ao 28.
44. Ordem estável em `factor_alternatives`: `sort_by_key` do Rust é estável como `std::stable_sort`, mesma chave (`firstLiteralCharacter`), mesma regra de barreira. Igual.

Divergências no código: nenhuma. Pendências reais: a camada que monta `groups` e `indices.groups` a partir de `output` (não portada) e as tabelas Unicode.

## Camada JS de RegExp (2026-10-08, auditoria por leitura, sem compilar): CORREÇÃO da pendência acima

A pendência "camada que monta `groups`/`indices.groups` não portada" está **errada**: a camada existe em `src/runtime`, fora de `src/yarr` (o `grep` anterior só olhou `src/yarr`). Nenhum arquivo foi editado nesta passada.

Mapa do que existe (todos conferidos contra `upstream/JavaScriptCore/runtime`, que fica em `wip/zjsc/upstream`, não na raiz):

- `reg_exp_matches_array.rs` (147 linhas): `create_reg_exp_matches_array` e `create_empty_reg_exp_matches_array`. Lido contra `createRegExpMatchesArrayWithGroupsOrIndices` do `RegExpMatchesArray.cpp`: mesma ordem de propriedades (`index`, `input`, `groups`, `indices`), `groups` e `indices.groups` com protótipo nulo, não participante vira `undefined`, par `[start, end]` por grupo, nomes duplicados via `subpattern_id_for_group_name(nome, ovector)` e `captureIndex > 0` (0 vira `undefined`), `indices.groups` na mesma ordem de `groups`. Única diferença declarada: sem `ensureGroupsStructure` (usa o ramo `putDirect` do C++ quando `groupsStructure` é nulo) e sem as `Structure`s pré-montadas; observável igual.
- `reg_exp_object.rs`: `last_index_as_unsigned`, `match_`, `test`, `exec_inline`, `exec` (semântica de `lastIndex` com global/sticky, `u32::MAX` como "passou do fim", `setLastIndex` antes do `recordMatch`, lastIndex não gravável lança). Igual a `RegExpObjectInlines.h`.
- `reg_exp_prototype.rs`: `source` com `escaped_pattern` (`/`, quebras de linha, colchetes, `(?:)`), `flags` genérico (ordem `dgimsuvy`), getters de flag, `toString`.
- `reg_exp_prototype_natives.rs` (1039 linhas): `compile`, `exec`, `test`, getters (`global`, `dotAll`, `hasIndices`, `ignoreCase`, `multiline`, `sticky`, `unicode`, `unicodeSets`, `source`, `flags`), `@@match`, `@@matchAll`, `@@replace` (com `get_substitution` e `groups` via `Get(result, "groups")`), `@@search`, `@@split` (species), `regExpExec` com `exec` customizado, e o construtor (`IsRegExp`, `Symbol.match`, `newTarget`, `RegExpCreate`, `@@species`).
- `reg_exp_legacy_natives.rs`: `RegExp.escape` e a tabela de 21 acessores legados (`input`/`$_`, `multiline`/`$*`, `lastMatch`/`$&`, `lastParen`/`$+`, `leftContext`/`` $` ``, `rightContext`/`$'`, `$1` a `$9`) com `CustomAccessor|ReadOnly|DontEnum`; `reg_exp_global_data.rs` e `string_regexp_globals.rs` guardam o último casamento.
- `js_reg_exp_string_iterator.rs` e `reg_exp_string_iterator_prototype.rs`: iterador de `matchAll`. `string_prototype_natives.rs` tem `replace`/`replaceAll`/`match`/`matchAll`/`search`/`split` com despacho por `Symbol.*` e `is_reg_exp`.

Casos conferidos de cabeça contra o bun 1.4.2 (todos coerentes com o código lido):

45. `/(?<a>a)|(?<a>b)/d.exec("b")`: `groups.a = "b"`, `indices.groups.a = [0,1]`, `m[1]` undefined. O código resolve `captureIndex` pelo ovector e lê `array`/`indices_array` nesse índice.
46. `/(?<x>a)(?<y>b)?/d.exec("a")`: `groups` `{x:"a", y:undefined}`, `indices.groups.y` undefined, `Object.getPrototypeOf(groups) === null`.
47. Sem `d`: `"indices" in m` é falso (propriedade só entra com `has_indices`); `create_empty_reg_exp_matches_array` põe `indices: undefined` só com `d`, como o C++.

Pendências reais da camada (nenhuma é bloqueio de exec/groups/indices):

1. `ensureGroupsStructure` e as `Structure`s de matches array (otimização, não observável).
2. `are_legacy_features_enabled` (`RegExp` de outro realm/subclasse): `record_match(..., false)` passa literal; conferir o ramo de subclasse contra `RegExpGlobalData::recordMatch` quando o golden de legados for escrito.
3. Falta golden específico de `RegExp.$1..$9`/`lastMatch`/`input` e de `@@replace` com `$<nome>` e função com `groups` como último argumento; os TSVs `regexp_v_bun.tsv` e `regexp_more_bun.tsv` cobrem só o motor, não a camada de legados.
4. O texto das pendências anteriores nesta nota ("camada JS fora de `src/yarr`") deve ser lido como "existe, falta golden", não "não portada".

## Golden de legados e `@@replace` (2026-10-08, sem compilar)

Criados `scripts/gen-regexp-legacy-golden.js` (bun 1.4.2), `tests/golden/regexp_legacy_bun.tsv` (1484 programas, 3 descartados por não terminarem no bun: getter de `global`/`flags` em `RegExp.prototype` com replace) e `tests/regexp_legacy_bun_golden.rs` (padrão de `function_error_bun_golden.rs`). Cobre `$1..$9`, `lastMatch`, `lastParen`, `leftContext`, `rightContext`, `input`, os aliases e `multiline`, depois de exec/test/match/replace/split/search/matchAll/`@@*`, falha mantendo o estado, `g`/`y`, subclasse, `Reflect.construct` com `newTarget` alheio, eval indireto, setters, descritores, `$<nome>` com e sem grupos, `$0`/`$00`/`$01`/`$10`, replacer função (com `groups` por último), `lastIndex` depois de replace, `replaceAll` não global, `split` com captura e limite, `matchAll` com `lastIndex` inicial e o iterador. O teste ainda NÃO foi rodado (sem cargo nesta passada): a lista de divergências sai do primeiro `cargo test regexp_legacy`.

Leitura de `record_match` e `are_legacy_features_enabled` contra `RegExpGlobalDataInlines.h`, `RegExpObjectInlines.h`, `RegExpConstructor.cpp` e `RegExpPrototype.cpp`:

- Correção da pendência 2: no JSC do upstream, `recordMatch` NÃO consulta `areLegacyFeaturesEnabled`. Subclasse e `Reflect.construct(RegExp, _, outro)` atualizam as estáticas normalmente; a flag só trava `compile` (`|this| RegExp object's legacy features are not enabled`). O Rust faz igual: `record_match` incondicional, `are_legacy_features_enabled` só em `reg_exp_proto_func_compile`. A frase do pedido "subclasse não atualiza" é a semântica da proposta TC39 (que o JSC ainda não implementa); o golden mede o que o bun faz.
- `areLegacyFeaturesEnabled(globalObject, newTarget)` do C++ (`newTarget` ausente ou igual ao `regExpConstructor` do realm): Rust em `reg_exp_prototype_natives.rs:886` compara com `callee`, equivalente quando o construtor chamado é o do realm.
- `RegExpPrototype.cpp` repete `recordMatch` do último casamento no `split` rápido (cache e `processSplit`). O Rust não tem esse caminho rápido: o `split` genérico passa por `exec`, que registra cada casamento; o último é o mesmo. Observável igual, a confirmar pelo golden de `split` + `L()`.
- Divergências no código: nenhuma achada por leitura, nenhuma edição em `src/`.
